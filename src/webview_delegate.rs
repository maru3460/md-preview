//! WKWebView のデリゲートの薄い層。判定は `md_preview::navigation` が持つ。
//!
//! wry の `with_navigation_handler` は URL しか渡さず、`with_ipc_handler` も送り元のフレームの
//! URL しか渡さない。外へ開くのをリンクのクリックに限るには移動の種類が、内線をメインフレーム
//! に限るには `isMainFrame` が要るので、その 2 か所だけ自分で受ける。
//!
//! - 移動: wry のナビゲーションのデリゲートを置き換えずに包む。wry のものは ivar に持ち、
//!   移動の判定（preferences 付きの版も含む）と応答の判定は自分で答えて決して転送しない。
//!   wry に要る通知（`didCommitNavigation` など）だけを名前を挙げて転送する
//! - 内線: wry には内線の受け口を渡さず、同じ名前（`ipc`）の受け口を自分で登録する。
//!   `window.ipc` を生やす script は、受け口の有無に関係なく wry が入れる
//!
//! 新しい窓は、ここではなく wry の UI のデリゲートが作らずにいる。wry 0.55.0 の
//! `webView:createWebViewWithConfiguration:forNavigationAction:windowFeatures:` は、
//! `new_window_req_handler` を渡していなければ nil を返す（`main.rs` は渡していない）。
//! 移動の判定でも行き先のフレームが無いものは取り消すので二重だが、UI のデリゲートは
//! 包んでいないので、wry がこの既定を変えるとそちらは効かなくなる。
//!
//! wry の中身（どのデリゲートのメソッドを持っているか、`ipc` という名前、UI のデリゲートの
//! 既定）に寄りかかるので、`Cargo.toml` で wry の版を固定している。上げるときはここを見直す。

use std::ffi::CStr;
use std::panic::AssertUnwindSafe;
use std::ptr::NonNull;

use md_preview::navigation::{self, NavDecision, NavFacts, NavKind, Target};
use objc2::rc::{autoreleasepool, Retained};
use objc2::runtime::{NSObject, ProtocolObject, Sel};
use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_foundation::{NSObjectProtocol, NSString, NSURLRequest};
use objc2_web_kit::{
    WKNavigation, WKNavigationAction, WKNavigationActionPolicy, WKNavigationDelegate,
    WKNavigationResponse, WKNavigationResponsePolicy, WKScriptMessage, WKScriptMessageHandler,
    WKUserContentController, WKWebView, WKWebpagePreferences,
};

/// wry が `window.ipc.postMessage` の行き先にしている受け口の名前。
const IPC_HANDLER_NAME: &str = "ipc";

/// [`install`] が返す札。**WKWebView の `navigationDelegate` は weak 参照**なので、
/// 呼び出し側が webview と同じだけ持ち続ける。落とすとデリゲートが nil に戻り、移動は
/// どれも WebKit の既定で通る（開いた側に倒れる）うえ、`didCommitNavigation` が wry へ
/// 届かなくなって Rust からページへの合図が止まる。
pub struct Hooks {
    _nav: Option<Retained<NavGuard>>,
    _ipc: Retained<IpcHandler>,
}

/// 移動の判定と内線の受け口を差し込む。`on_ipc` には、メインフレームの md の画面から来た
/// メッセージの本文だけが渡る。
///
/// webview を作った直後（イベントループを回す前）に呼ぶ。wry は `build()` の中で最初の
/// ページの読み込みを始めるが、判定も内線もメインスレッドへ戻ってからしか届かない。
pub fn install(webview: &wry::WebView, on_ipc: impl Fn(&str) + 'static) -> Hooks {
    use wry::WebViewExtMacOS;

    let mtm = MainThreadMarker::new().expect("must be on main thread");
    let wv = webview.webview();

    // SAFETY: navigationDelegate / setNavigationDelegate は WKWebView の公開の口で、
    // メインスレッドから呼ぶ。
    let nav = match unsafe { wv.navigationDelegate() } {
        Some(inner) => {
            let guard = NavGuard::new(mtm, inner);
            unsafe { wv.setNavigationDelegate(Some(ProtocolObject::from_ref(&*guard))) };
            Some(guard)
        }
        // wry は必ず自分のデリゲートを付けるので、来ないはず。包めなければ wry の
        // デリゲートと `with_navigation_handler` に任せる（そちらは外へ何も開かず、
        // サブフレームも閉じる側で答える）。来たら wry の作りが変わったということなので、
        // 開発中のビルドではその場で落として気付く。
        None => {
            eprintln!("md: ナビゲーションのデリゲートが無く、移動の判定を差し込めなかった");
            debug_assert!(false, "wry のナビゲーションのデリゲートが無い");
            None
        }
    };

    let ipc = IpcHandler::new(mtm, Box::new(on_ipc));
    // 同じ名前の受け口が既にあると NSInvalidArgumentException になる。wry には内線の
    // 受け口を渡していないので `ipc` は空いているはずだが、ObjC の例外は Rust を巻き戻せず
    // 素通しにするとプロセスごと理由も残さず落ちるので、wry と同じく catch で受けて名指しで落とす。
    // Why not 受け口無しで続ける: 窓は出ても閉じる・転送・メニューが全部効かない。
    // SAFETY: configuration と userContentController は公開の口で、メインスレッドから呼ぶ。
    let added = objc2::exception::catch(AssertUnwindSafe(|| unsafe {
        let controller = wv.configuration().userContentController();
        controller.addScriptMessageHandler_name(
            ProtocolObject::from_ref(&*ipc),
            &NSString::from_str(IPC_HANDLER_NAME),
        );
    }));
    if let Err(e) = added {
        panic!("内線の受け口 `{IPC_HANDLER_NAME}` を登録できなかった（wry が同じ名前を使っている？）: {e:?}");
    }

    Hooks { _nav: nav, _ipc: ipc }
}

fn url_of(request: &NSURLRequest) -> Option<String> {
    request.URL().and_then(|u| u.absoluteString()).map(|s| s.to_string())
}

/// 移動の判定。答えを決めるのは [`navigation::decide`] で、ここは事実を集めて開くだけ。
fn policy_for(action: &WKNavigationAction) -> WKNavigationActionPolicy {
    // ダウンロードは受けない（受け口も持たない）。wry も受け口が無ければ取り消している。
    // shouldPerformDownload は macOS 11.3 から。
    // SAFETY: どれも WKNavigationAction（と WKFrameInfo）の公開の口。
    let can_ask: bool = unsafe { msg_send![action, respondsToSelector: sel!(shouldPerformDownload)] };
    if can_ask && unsafe { action.shouldPerformDownload() } {
        return WKNavigationActionPolicy::Cancel;
    }
    let Some(url) = url_of(&*unsafe { action.request() }) else {
        return WKNavigationActionPolicy::Cancel;
    };
    let kind = NavKind::from_raw(unsafe { action.navigationType() }.0);
    let target = match unsafe { action.targetFrame() } {
        Some(frame) if unsafe { frame.isMainFrame() } => Target::Main,
        Some(_) => Target::Sub,
        None => Target::NewWindow,
    };
    match navigation::decide(NavFacts { url: &url, kind, target }) {
        NavDecision::Allow => WKNavigationActionPolicy::Allow,
        NavDecision::Cancel => WKNavigationActionPolicy::Cancel,
        NavDecision::OpenInBrowser => {
            open_in_browser(&url);
            WKNavigationActionPolicy::Cancel
        }
    }
}

/// 既定のブラウザで開く。内線の `open-url:`（`.html` のプレビューの中の外部リンク）もここを通る。
pub fn open_in_browser(url: &str) {
    // 呼ぶ側が http(s) だけを渡す（`decide` の OpenInBrowser と `open_url_allowed`）ので、
    // `open` の引数が `-` で始まってオプションに化けることは無い。
    std::process::Command::new("open").arg(url).spawn().ok();
}

pub struct NavGuardIvars {
    inner: Retained<ProtocolObject<dyn WKNavigationDelegate>>,
}

define_class!(
    // SAFETY:
    // - 親の NSObject はサブクラス化に条件を課さない。
    // - Drop を実装しないので dealloc の生成も要らない。
    #[unsafe(super(NSObject))]
    // WebKit はデリゲートをメインスレッドからしか呼ばない。
    #[thread_kind = MainThreadOnly]
    #[ivars = NavGuardIvars]
    pub struct NavGuard;

    unsafe impl NSObjectProtocol for NavGuard {}

    // Why not respondsToSelector: と forwardingTargetForSelector: で wry へ丸ごと流す:
    // 知らないメソッドまで wry へ届き、移動の判定を wry に答えさせる道が残る。wry の版を
    // 固定しているので、wry が持っている通知を名前で挙げて転送する方が、通る道が読める。
    unsafe impl WKNavigationDelegate for NavGuard {
        #[unsafe(method(webView:decidePolicyForNavigationAction:decisionHandler:))]
        fn decide_action(
            &self,
            _webview: &WKWebView,
            action: &WKNavigationAction,
            handler: &block2::DynBlock<dyn Fn(WKNavigationActionPolicy)>,
        ) {
            handler.call((policy_for(action),));
        }

        // これを持つと、WebKit は上の preferences 無しの版を呼ばなくなる。wry は持って
        // いないが、持たせておけば将来 wry がこちらを実装しても判定が wry へ戻らない。
        #[unsafe(method(webView:decidePolicyForNavigationAction:preferences:decisionHandler:))]
        fn decide_action_with_preferences(
            &self,
            _webview: &WKWebView,
            action: &WKNavigationAction,
            preferences: &WKWebpagePreferences,
            handler: &block2::DynBlock<dyn Fn(WKNavigationActionPolicy, NonNull<WKWebpagePreferences>)>,
        ) {
            handler.call((policy_for(action), NonNull::from(preferences)));
        }

        // wry は、ダウンロードの受け口を持たないときは常に Allow を返す。同じ答えを自分で返す。
        #[unsafe(method(webView:decidePolicyForNavigationResponse:decisionHandler:))]
        fn decide_response(
            &self,
            _webview: &WKWebView,
            _response: &WKNavigationResponse,
            handler: &block2::DynBlock<dyn Fn(WKNavigationResponsePolicy)>,
        ) {
            handler.call((WKNavigationResponsePolicy::Allow,));
        }

        // wry はここで、ページの読み込み前に積まれた evaluate_script を流す。転送しないと
        // ベル・転送・テーマの切り替えなど、Rust からページへの合図が全部止まる。
        #[unsafe(method(webView:didCommitNavigation:))]
        fn did_commit(&self, webview: &WKWebView, navigation: Option<&WKNavigation>) {
            let s = sel!(webView:didCommitNavigation:);
            if self.inner_responds(s) {
                let _: () = unsafe { msg_send![&*self.ivars().inner, webView: webview, didCommitNavigation: navigation] };
            }
        }

        #[unsafe(method(webView:didFinishNavigation:))]
        fn did_finish(&self, webview: &WKWebView, navigation: Option<&WKNavigation>) {
            let s = sel!(webView:didFinishNavigation:);
            if self.inner_responds(s) {
                let _: () = unsafe { msg_send![&*self.ivars().inner, webView: webview, didFinishNavigation: navigation] };
            }
        }

        #[unsafe(method(webViewWebContentProcessDidTerminate:))]
        fn content_process_terminated(&self, webview: &WKWebView) {
            let s = sel!(webViewWebContentProcessDidTerminate:);
            if self.inner_responds(s) {
                let _: () = unsafe { msg_send![&*self.ivars().inner, webViewWebContentProcessDidTerminate: webview] };
            }
        }
    }
);

impl NavGuard {
    fn new(
        mtm: MainThreadMarker,
        inner: Retained<ProtocolObject<dyn WKNavigationDelegate>>,
    ) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(NavGuardIvars { inner });
        unsafe { msg_send![super(this), init] }
    }

    fn inner_responds(&self, s: Sel) -> bool {
        // SAFETY: respondsToSelector: は NSObject の公開の口。
        unsafe { msg_send![&*self.ivars().inner, respondsToSelector: s] }
    }
}

pub struct IpcHandlerIvars {
    on_message: Box<dyn Fn(&str)>,
}

define_class!(
    // SAFETY: NavGuard と同じ。
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = IpcHandlerIvars]
    pub struct IpcHandler;

    unsafe impl NSObjectProtocol for IpcHandler {}

    unsafe impl WKScriptMessageHandler for IpcHandler {
        #[unsafe(method(userContentController:didReceiveScriptMessage:))]
        fn did_receive(&self, _controller: &WKUserContentController, message: &WKScriptMessage) {
            // SAFETY: frameInfo / isMainFrame / request / body は公開の口。
            let frame = unsafe { message.frameInfo() };
            let is_main = unsafe { frame.isMainFrame() };
            let url = url_of(&*unsafe { frame.request() }).unwrap_or_default();
            if !navigation::ipc_from_app(is_main, &url) {
                return;
            }
            // 本文は文字列だけ受ける（wry と同じ）。
            let Ok(body) = unsafe { message.body() }.downcast::<NSString>() else { return };
            // 対になっていないサロゲート（JS の文字列には作れる）を含むと UTF-8 にできず、
            // UTF8String が NULL を返す。正規の内線は作らない形なので捨てる。
            // Why not `to_string()`: NULL を確かめずに読みにいく。
            // UTF8String の中身は autorelease されるので、読む間だけの pool で包む。
            autoreleasepool(|_| {
                let ptr = body.UTF8String();
                if ptr.is_null() {
                    return;
                }
                // SAFETY: NULL でない UTF8String は NUL 終端で、この pool が抜けるまで
                // （`body` も生きている）有効。
                let Ok(text) = unsafe { CStr::from_ptr(ptr) }.to_str() else { return };
                (self.ivars().on_message)(text);
            });
        }
    }
);

impl IpcHandler {
    fn new(mtm: MainThreadMarker, on_message: Box<dyn Fn(&str)>) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(IpcHandlerIvars { on_message });
        unsafe { msg_send![super(this), init] }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use objc2::runtime::AnyClass;
    use objc2::ClassType;

    /// 開発中のビルドでは、クラスを登録する時点でメソッドの型が WebKit の宣言と照らされる。
    /// 窓を出さずに、型の食い違いをここで落とす。
    #[test]
    fn the_navigation_guard_answers_exactly_the_methods_it_owns() {
        let cls: &AnyClass = NavGuard::class();
        for s in [
            sel!(webView:decidePolicyForNavigationAction:decisionHandler:),
            sel!(webView:decidePolicyForNavigationAction:preferences:decisionHandler:),
            sel!(webView:decidePolicyForNavigationResponse:decisionHandler:),
            sel!(webView:didCommitNavigation:),
            sel!(webView:didFinishNavigation:),
            sel!(webViewWebContentProcessDidTerminate:),
        ] {
            assert!(cls.responds_to(s), "{s:?}");
        }
        // 持っていない通知は wry へも流れない（WebKit は呼ばない）。
        assert!(!cls.responds_to(sel!(webView:navigationAction:didBecomeDownload:)));
        assert!(!cls.responds_to(sel!(webView:didStartProvisionalNavigation:)));
    }

    #[test]
    fn the_ipc_handler_registers_as_a_script_message_handler() {
        let cls: &AnyClass = IpcHandler::class();
        assert!(cls.responds_to(sel!(userContentController:didReceiveScriptMessage:)));
    }
}
