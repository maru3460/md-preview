//! WKWebView のデリゲートの薄い層。判定は `md_preview::navigation` が持つ。
//!
//! wry の `with_navigation_handler` は URL しか渡さず、`with_ipc_handler` も送り元のフレームの
//! URL しか渡さない。外へ開くのをリンクのクリックに限るには移動の種類と送り元のフレームが、
//! 内線をメインフレームに限るには `isMainFrame` が要るので、そこを自分で受ける。
//!
//! - 移動: wry のナビゲーションのデリゲートを置き換えずに包む。wry のものは ivar に持ち、
//!   移動の判定（preferences 付きの版も含む）と応答の判定は自分で答えて決して転送しない。
//!   wry に要る通知（`didCommitNavigation` など）だけを名前を挙げて転送する
//! - 内線: wry には内線の受け口を渡さず、同じ名前（`ipc`）の受け口を自分で登録する。
//!   `window.ipc` を生やす script は、受け口の有無に関係なく wry が入れる
//! - 新しい窓・カメラとマイク: wry の UI のデリゲートも同じ要領で包み、カメラとマイクの許可は
//!   自分で断る（wry は尋ねずに許す）。窓を作る口
//!   （`webView:createWebViewWithConfiguration:forNavigationAction:windowFeatures:`）は
//!   自分で nil を返す。wry 0.55.0 も `new_window_req_handler` を渡さなければ nil を返すが、
//!   その既定に任せない。移動の判定でも行き先のフレームが無いものは取り消すので二重になる
//!
//! wry の中身（どのデリゲートのメソッドを持っているか、`ipc` という名前）に寄りかかるので、
//! `Cargo.toml` で wry の版を固定している。上げるときはここを見直す。

use std::cell::Cell;
use std::ffi::CStr;
use std::panic::AssertUnwindSafe;
use std::ptr::NonNull;

use md_preview::navigation::{self, NavDecision, NavFacts, NavKind, Source, Target};
use objc2::rc::{autoreleasepool, Retained};
use objc2::runtime::{NSObject, ProtocolObject, Sel};
use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::NSWorkspace;
use objc2_foundation::{NSArray, NSObjectProtocol, NSString, NSURL, NSURLRequest};
use objc2_web_kit::{
    WKFrameInfo, WKMediaCaptureType, WKNavigation, WKNavigationAction, WKNavigationActionPolicy,
    WKNavigationDelegate, WKNavigationResponse, WKNavigationResponsePolicy, WKOpenPanelParameters,
    WKPermissionDecision, WKScriptMessage, WKScriptMessageHandler, WKSecurityOrigin, WKUIDelegate,
    WKUserContentController, WKWebView, WKWebViewConfiguration, WKWebpagePreferences,
    WKWindowFeatures,
};

/// wry が `window.ipc.postMessage` の行き先にしている受け口の名前。
const IPC_HANDLER_NAME: &str = "ipc";

/// [`install`] が返す札。**WKWebView の `navigationDelegate` は weak 参照**なので、
/// 呼び出し側が webview と同じだけ持ち続ける。落とすとデリゲートが nil に戻り、移動は
/// どれも WebKit の既定で通る（開いた側に倒れる）うえ、`didCommitNavigation` が wry へ
/// 届かなくなって Rust からページへの合図が止まる。
/// UI のデリゲート（`UIDelegate`）も weak 参照なので同じ。
pub struct Hooks {
    _nav: Option<Retained<NavGuard>>,
    _ui: Option<Retained<UiGuard>>,
    _ipc: Retained<IpcHandler>,
}

/// 移動の判定と内線の受け口を差し込む。`on_ipc` には、メインフレームの md の画面から来た
/// メッセージの本文だけが渡る。`on_notice` には、移動の判定が押した人に知らせたい文言
/// （[`NavDecision::Unopenable`]）が渡る。
///
/// webview を作った直後（イベントループを回す前）に呼ぶ。wry は `build()` の中で最初の
/// ページの読み込みを始めるが、判定も内線もメインスレッドへ戻ってからしか届かない。
pub fn install(
    webview: &wry::WebView,
    on_ipc: impl Fn(&str) + 'static,
    on_notice: impl Fn(&str) + 'static,
) -> Hooks {
    use wry::WebViewExtMacOS;

    let mtm = MainThreadMarker::new().expect("must be on main thread");
    let wv = webview.webview();

    // SAFETY: navigationDelegate / setNavigationDelegate は WKWebView の公開の口で、
    // メインスレッドから呼ぶ。
    let nav = match unsafe { wv.navigationDelegate() } {
        Some(inner) => {
            let guard = NavGuard::new(mtm, inner, Box::new(on_notice));
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

    // SAFETY: UIDelegate / setUIDelegate は WKWebView の公開の口で、メインスレッドから呼ぶ。
    let ui = match unsafe { wv.UIDelegate() } {
        Some(inner) => {
            let guard = UiGuard::new(mtm, inner);
            unsafe { wv.setUIDelegate(Some(ProtocolObject::from_ref(&*guard))) };
            Some(guard)
        }
        // 包めなくても、窓を作る移動は移動の判定が取り消す（行き先のフレームが無い）。
        None => {
            eprintln!("md: UI のデリゲートが無く、新しい窓の拒否を差し込めなかった");
            debug_assert!(false, "wry の UI のデリゲートが無い");
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

    Hooks { _nav: nav, _ui: ui, _ipc: ipc }
}

fn url_of(request: &NSURLRequest) -> Option<String> {
    request.URL().and_then(|u| u.absoluteString()).map(|s| s.to_string())
}

/// 移動の判定。答えを決めるのは [`navigation::decide`] で、ここは事実を集めて開くだけ。
/// `main_committed` は、メインフレームが最初の読み込みを終えたか（[`NavGuard`] が数える）。
fn policy_for(action: &WKNavigationAction, guard: &NavGuardIvars) -> WKNavigationActionPolicy {
    let main_committed = guard.main_committed.get();
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
    let target_frame = unsafe { action.targetFrame() };
    let target = match &target_frame {
        Some(frame) if unsafe { frame.isMainFrame() } => Target::Main,
        Some(_) => Target::Sub,
        None => Target::NewWindow,
    };
    let target_url = target_frame.as_deref().and_then(frame_url).unwrap_or_default();
    // 宣言では nil にならないが、来ることがある（objc2 も Option で受ける）。
    let source_frame = unsafe { action.sourceFrame() };
    let source_url = source_frame.as_deref().and_then(frame_url).unwrap_or_default();
    let source = match &source_frame {
        Some(frame) if unsafe { frame.isMainFrame() } => Source::Main,
        Some(_) => Source::Sub(&source_url),
        None => Source::Unknown,
    };
    let facts = NavFacts { url: &url, kind, target, target_url: &target_url, source, main_committed };
    match navigation::decide(facts) {
        NavDecision::Allow => WKNavigationActionPolicy::Allow,
        NavDecision::Cancel => WKNavigationActionPolicy::Cancel,
        NavDecision::OpenInBrowser => {
            open_in_browser(&url);
            WKNavigationActionPolicy::Cancel
        }
        NavDecision::OpenExternally => {
            open_externally(&url);
            WKNavigationActionPolicy::Cancel
        }
        NavDecision::Unopenable => {
            (guard.on_notice)(navigation::UNOPENABLE_LINK_MSG);
            WKNavigationActionPolicy::Cancel
        }
    }
}

/// フレームがいま出している文書の URL。
fn frame_url(frame: &WKFrameInfo) -> Option<String> {
    // SAFETY: request は WKFrameInfo の公開の口。
    url_of(&*unsafe { frame.request() })
}

/// 既定のブラウザで開く。内線の `open-url:`（`.html` のプレビューの中の外部リンク）もここを通る。
pub fn open_in_browser(url: &str) {
    // 呼ぶ側が http(s) だけを渡す（`decide` の OpenInBrowser と `open_url_allowed`）ので、
    // `open` の引数が `-` で始まってオプションに化けることは無い。
    std::process::Command::new("open").arg(url).spawn().ok();
}

/// 外部スキームを受け持つアプリで開く。呼ぶ側が `navigation::external_allowed` を通したもの
/// だけを渡す（`decide` の OpenExternally と、内線の `open-url:` の `open_url_decision`）。
///
/// Why not `open_in_browser` と同じく `open` コマンドへ渡す: 子プロセスを挟む理由が無く、
/// 引数の読まれ方（`-` で始まる文字列など）を気にしなくて済む。
pub fn open_externally(url: &str) {
    let Some(url) = NSURL::URLWithString(&NSString::from_str(url)) else { return };
    // 開けなかったとき（受け持つアプリが無いなど）は何もしない。押しても何も起きないだけ。
    let _ = NSWorkspace::sharedWorkspace().openURL(&url);
}

pub struct NavGuardIvars {
    inner: Retained<ProtocolObject<dyn WKNavigationDelegate>>,
    /// メインフレームの最初の読み込みを終えたか。`didCommitNavigation` で立て、中身のプロセスが
    /// 落ちたら下ろす（落ちた後の読み込みは、また最初の読み込みになる）。
    main_committed: Cell<bool>,
    /// 押した人に知らせる文言の届け先（トースト）。
    on_notice: Box<dyn Fn(&str)>,
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
            handler.call((policy_for(action, self.ivars()),));
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
            handler.call((policy_for(action, self.ivars()), NonNull::from(preferences)));
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
        // ナビゲーションのデリゲートの通知はメインフレームのものだけ来る。
        fn did_commit(&self, webview: &WKWebView, navigation: Option<&WKNavigation>) {
            self.ivars().main_committed.set(true);
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
            self.ivars().main_committed.set(false);
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
        on_notice: Box<dyn Fn(&str)>,
    ) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(NavGuardIvars { inner, main_committed: Cell::new(false), on_notice });
        unsafe { msg_send![super(this), init] }
    }

    fn inner_responds(&self, s: Sel) -> bool {
        // SAFETY: respondsToSelector: は NSObject の公開の口。
        unsafe { msg_send![&*self.ivars().inner, respondsToSelector: s] }
    }
}

pub struct UiGuardIvars {
    inner: Retained<ProtocolObject<dyn WKUIDelegate>>,
}

define_class!(
    // SAFETY: NavGuard と同じ。
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = UiGuardIvars]
    pub struct UiGuard;

    unsafe impl NSObjectProtocol for UiGuard {}

    // 許可制。持つのは次の 3 つだけで、wry へ渡すのはファイルの選択だけ。
    // - 窓を作る口: nil（新しい窓は作らない）
    // - カメラ・マイクの許可: 断る
    // - ファイルの選択（`.html` の `<input type=file>`）: wry の NSOpenPanel へ渡す
    // ほかの口（JS の alert / confirm / prompt、窓を閉じる、など）は持たないので、WebKit は
    // デリゲートが無いときの既定で振る舞う。
    // Why not NavGuard のように wry が持つ口を全部転送する: wry の版を上げて UI の機能
    // （ダイアログや許可の口）が増えたとき、ここを見直さないまま文書から届くようになる。
    // 必要なものだけを名前で足す。
    unsafe impl WKUIDelegate for UiGuard {
        #[unsafe(method_id(webView:createWebViewWithConfiguration:forNavigationAction:windowFeatures:))]
        fn create_web_view(
            &self,
            _webview: &WKWebView,
            _configuration: &WKWebViewConfiguration,
            _action: &WKNavigationAction,
            _features: &WKWindowFeatures,
        ) -> Option<Retained<WKWebView>> {
            None
        }

        // `.html` の `<input type=file>`。wry が NSOpenPanel を出す。
        #[unsafe(method(webView:runOpenPanelWithParameters:initiatedByFrame:completionHandler:))]
        fn run_open_panel(
            &self,
            webview: &WKWebView,
            parameters: &WKOpenPanelParameters,
            frame: &WKFrameInfo,
            handler: &block2::DynBlock<dyn Fn(*mut NSArray<NSURL>)>,
        ) {
            let s = sel!(webView:runOpenPanelWithParameters:initiatedByFrame:completionHandler:);
            if self.inner_responds(s) {
                let _: () = unsafe {
                    msg_send![&*self.ivars().inner, webView: webview, runOpenPanelWithParameters: parameters, initiatedByFrame: frame, completionHandler: handler]
                };
            } else {
                handler.call((std::ptr::null_mut(),));
            }
        }

        // カメラ・マイクは常に断る。プレビューに要る場面が無い。
        // Why not wry へ転送する: wry 0.55.0 は尋ねずに Grant を返すので、文書（や外部の
        // iframe）が黙ってカメラ・マイクを使えてしまう。
        // Why not 口ごと持たない: 持たなければ WebKit の既定（Prompt）になり、文書が許可の
        // ダイアログを出させられる。
        #[unsafe(method(webView:requestMediaCapturePermissionForOrigin:initiatedByFrame:type:decisionHandler:))]
        fn request_media_capture(
            &self,
            _webview: &WKWebView,
            _origin: &WKSecurityOrigin,
            _frame: &WKFrameInfo,
            _capture_type: WKMediaCaptureType,
            handler: &block2::DynBlock<dyn Fn(WKPermissionDecision)>,
        ) {
            handler.call((WKPermissionDecision::Deny,));
        }
    }
);

impl UiGuard {
    fn new(mtm: MainThreadMarker, inner: Retained<ProtocolObject<dyn WKUIDelegate>>) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(UiGuardIvars { inner });
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

    /// UI のデリゲートが持つのは、窓を作る口（断る）・カメラとマイクの許可（断る）・ファイルの
    /// 選択（wry へ渡す）の 3 つだけ。カメラ・マイクの口は持たないと WebKit の既定（許可の
    /// ダイアログ）になるので、持っていること自体を確かめる。
    #[test]
    fn the_ui_guard_implements_only_new_windows_media_capture_and_the_file_panel() {
        let cls: &AnyClass = UiGuard::class();
        for s in [
            sel!(webView:createWebViewWithConfiguration:forNavigationAction:windowFeatures:),
            sel!(webView:runOpenPanelWithParameters:initiatedByFrame:completionHandler:),
            sel!(webView:requestMediaCapturePermissionForOrigin:initiatedByFrame:type:decisionHandler:),
        ] {
            assert!(cls.responds_to(s), "{s:?}");
        }
        // ほかの口は持たない（WebKit のデリゲートが無いときの既定のまま）。
        for s in [
            sel!(webView:runJavaScriptAlertPanelWithMessage:initiatedByFrame:completionHandler:),
            sel!(webView:runJavaScriptConfirmPanelWithMessage:initiatedByFrame:completionHandler:),
            sel!(webView:runJavaScriptTextInputPanelWithPrompt:defaultText:initiatedByFrame:completionHandler:),
            sel!(webViewDidClose:),
            sel!(webView:requestDeviceOrientationAndMotionPermissionForOrigin:initiatedByFrame:decisionHandler:),
            sel!(webView:contextMenuDidEndForElement:),
        ] {
            assert!(!cls.responds_to(s), "{s:?}");
        }
    }

    #[test]
    fn the_ipc_handler_registers_as_a_script_message_handler() {
        let cls: &AnyClass = IpcHandler::class();
        assert!(cls.responds_to(sel!(userContentController:didReceiveScriptMessage:)));
    }
}
