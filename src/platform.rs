use std::path::Path;

/// クリップボードへの書き込み。ページ側の `MdCommon.copyText` が
/// navigator.clipboard を使えなかった時の受け皿で、絶対パスのコピーもここへ来る。
/// IPC ハンドラ（macOS では WKWebView がメインスレッドで配信）から呼ぶ前提。
#[cfg(target_os = "macos")]
pub fn copy_to_clipboard(text: &str) {
    use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString};
    use objc2_foundation::NSString;
    let pb = NSPasteboard::generalPasteboard();
    let ns = NSString::from_str(text);
    unsafe {
        pb.clearContents();
        pb.setString_forType(&ns, NSPasteboardTypeString);
    }
}

/// Finder で対象ファイルを選択表示する（Reveal in Finder）。
#[cfg(target_os = "macos")]
pub fn reveal_in_finder(path: &Path) {
    std::process::Command::new("open").arg("-R").arg(path).spawn().ok();
}

/// 既定アプリでファイルを開く。実行系拡張子の弾きは呼び出し側で行う。
#[cfg(target_os = "macos")]
pub fn open_default(path: &Path) {
    std::process::Command::new("open").arg(path).spawn().ok();
}

#[cfg(not(target_os = "macos"))]
pub fn copy_to_clipboard(_text: &str) {}

#[cfg(not(target_os = "macos"))]
pub fn reveal_in_finder(_path: &Path) {}

#[cfg(not(target_os = "macos"))]
pub fn open_default(_path: &Path) {}

/// OS がダークモードか。テーマが OS 追従（[`md_preview::theme::Appearance::Auto`]）の
/// ときだけ、窓の下地色をどちらへ寄せるかの判断に使う。
///
/// 判定できなければ None。当てずっぽうで「ライト」を返すと、ダークな OS で白い板を
/// 出すことになり、窓を先に出す変更でいちばん避けたかった見え方になる。
/// 受け手（[`md_preview::theme::window_bg`]）が None を色なしへ伝えるのは、テーマが
/// OS 追従のときだけ。外観を固定したテーマは OS 設定を見ないので、ここが None でも
/// 色は決まる。
///
/// 見るのは `NSApp` の実効 appearance。`NSUserDefaults` の `AppleInterfaceStyle` は
/// アプリ側の上書きを映さないし、WKWebView の `prefers-color-scheme` が降りてくる
/// 元とも切れてしまう。同じ実効 appearance を見ておけば下地とページが食い違わない。
///
/// **イベントループを作った後に呼ぶこと。** `sharedApplication` は `NSApp` が
/// 居なければ自分で作ってしまうが、tao は最初の `sharedApplication` で自前の
/// `NSApplication` サブクラスを焼き込む。先に呼ぶとそれを奪って、tao のイベント
/// 処理が壊れる。
///
/// 名前の直接比較ではなく `bestMatchFromAppearancesWithNames` を通す。
/// アクセシビリティ設定で `NSAppearanceNameAccessibilityHighContrastDarkAqua` に
/// なることがあり、直接比較ではそれを「ライト」と取り違える。
#[cfg(target_os = "macos")]
pub fn os_is_dark() -> Option<bool> {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSApplication};
    use objc2_foundation::NSArray;

    let mtm = MainThreadMarker::new()?;
    let appearance = NSApplication::sharedApplication(mtm).effectiveAppearance();
    // unsafe が要るのは extern static の読み出しだけ。配列の生成も比較も safe。
    let (aqua, dark) = unsafe { (NSAppearanceNameAqua, NSAppearanceNameDarkAqua) };
    let names = NSArray::from_slice(&[aqua, dark]);
    // どちらにも寄らなかったときも「判らない」を通す。false（＝ライト）を返すと、
    // ダークな OS で白い板を出すという、この関数が避けようとしている形になる。
    appearance
        .bestMatchFromAppearancesWithNames(&names)
        .map(|best| best.isEqualToString(dark))
}

/// macOS 以外に OS の外観を読む手立てを持たない。色を決めずに窓の既定へ任せる。
#[cfg(not(target_os = "macos"))]
pub fn os_is_dark() -> Option<bool> {
    None
}

/// 窓そのものの外観（タイトルバーと信号ボタン）をテーマに合わせる。
///
/// 背景色を塗るだけでは足りない。地色は変わってもタイトル文字と信号ボタンは OS の
/// 外観のまま残るので、ダークなテーマをライトな OS で開くと、暗いタイトルバーに
/// ライト用の部品が乗ってちぐはぐになる。
///
/// **窓の中の描画にも及ぶ。** WKWebView は窓の `NSAppearance` を継承するので、ページが
/// 見る `prefers-color-scheme` が OS ではなくテーマ側になる。効くのは html を描く
/// iframe（`base.css` の `.html-frame`）と、`prefers-color-scheme` を見る同梱ライブラリ
/// （draw.io の自動ダーク）。窓全体の見え方を揃えるための意図した挙動で、`.html-frame`
/// 側のコメントもそう書いてある。
///
/// 呼ぶのは外観を固定したテーマのときだけ。OS 追従のテーマでは OS の設定がそのまま
/// 正解なので、何も指定せず OS に任せる。
///
/// **tao の `Window::set_theme` は使えない。** あちらの実装は `NSApp` へ `setAppearance`
/// するので、窓ではなくアプリ全体の外観が変わる。[`os_is_dark`] が読む実効 appearance
/// まで固定値に化けて、OS の設定を知る手段が無くなる。
///
/// 前提は「起動時に一度だけ呼ぶ」こと。実行中にテーマを切り替えられるようにする
/// とき（#38）は、固定テーマから OS 追従へ戻す経路で `setAppearance(None)` を呼んで
/// 指定を外す必要がある。呼ばないと古い固定外観が残る。
///
/// 外観固定テーマの窓は、アクセシビリティの「コントラストを上げる」にも追随しなく
/// なる（素の `NSAppearanceNameDarkAqua` で固定するため）。テーマが外観を決める以上は
/// 筋が通るが、[`os_is_dark`] が高コントラスト版まで拾うのと非対称なのは承知の上。
#[cfg(target_os = "macos")]
pub fn set_window_appearance(window: &tao::window::Window, dark: bool) {
    use objc2_app_kit::{
        NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua, NSAppearanceNameDarkAqua,
        NSWindow,
    };
    use tao::platform::macos::WindowExtMacOS;

    let ptr = window.ns_window() as *mut NSWindow;
    // 引き受けている不変条件は 2 つ。tao が「Window が生きている間だけ有効」と言って
    // いる生ポインタであること（呼ぶのは窓を作った直後だけなので生きている）と、
    // `NSWindow` がメインスレッド専用であること（tao のイベントループがメインスレッド
    // でしか回らないので満たされる）。
    let Some(ns_window) = (unsafe { ptr.as_ref() }) else {
        return;
    };
    let name = unsafe {
        if dark {
            NSAppearanceNameDarkAqua
        } else {
            NSAppearanceNameAqua
        }
    };
    ns_window.setAppearance(NSAppearance::appearanceNamed(name).as_deref());
}

#[cfg(not(target_os = "macos"))]
pub fn set_window_appearance(_window: &tao::window::Window, _dark: bool) {}

/// 窓がネイティブ全画面に入っているか。**「閉じる前に抜けるのを待つ必要があるか」の
/// 判定だけに使う**（#59）。
///
/// ⚠️ **抜け終わったかの判定には使えない。** このビットは抜け*始め*で落ちる
/// （`set_fullscreen(None)` の 17ms 後には false、実測 2026-09-23）。アニメーションは
/// そこから 1 秒近く続くので、これを完了の合図に使うと結局「全画面のまま終了」になる。
/// 完了は [`watch_exit_fullscreen`] の通知で受けること。
///
/// **tao の `Window::fullscreen()` も使えない。** あちらが返すのは tao 自前の状態で、
/// `set_fullscreen(None)` は `toggleFullScreen:` を main queue へ積む**前**にその状態を
/// 書き換える（tao 0.35 `platform_impl/macos/window.rs`）。頼んだ瞬間に `None` になる。
///
/// 緑ボタン・⌃⌘F（`setup_menu` の `toggleFullScreen:`）・`set_fullscreen` のどれで
/// 入っても同じビットが立つ。入り口を問わないのが AppKit の実体を読む利点。
#[cfg(target_os = "macos")]
pub fn is_window_fullscreen(window: &tao::window::Window) -> bool {
    use objc2_app_kit::{NSWindow, NSWindowStyleMask};
    use tao::platform::macos::WindowExtMacOS;

    let ptr = window.ns_window() as *mut NSWindow;
    // 引き受けている不変条件は [`set_window_appearance`] と同じ 2 つだが、生存の根拠は
    // 違う。こちらは閉じる経路から呼ぶので「窓を作った直後だから生きている」とは言えない。
    // 代わりに、借りている `window` が生きている間はこのポインタも有効（tao の契約）で、
    // 呼び出し側はイベントループのクロージャが所有する `window` を渡している、を根拠にする。
    let Some(ns_window) = (unsafe { ptr.as_ref() }) else {
        return false;
    };
    ns_window.styleMask().contains(NSWindowStyleMask::FullScreen)
}

/// #59 は「全画面の窓を閉じると別の Space が出てくる」という macOS 固有の症状で、
/// 他の OS には全画面 Space に相当するものが無い。待つ必要が無いので常に false。
#[cfg(not(target_os = "macos"))]
pub fn is_window_fullscreen(_window: &tao::window::Window) -> bool {
    false
}

/// 通知の購読が返す札。**購読を解除する手段は持たない。**
///
/// 解除に要るのは `removeObserver:` で、札を落とすだけでは切れない（通知センターが
/// 自分でも保持している）。それでも `Drop` を書かないのは、**走らないから**——
/// `EventLoop::run` は終了時に `process::exit` するので、この札も含めて `Drop` は
/// 一度も呼ばれない（`instance::Handle` が同じ理由で `Drop` を持たないのと揃える）。
/// 窓も購読もプロセスと寿命を揃えるものなので、解除する場面がそもそも無い。
#[cfg(target_os = "macos")]
pub struct NotificationWatch(
    #[allow(dead_code)]
    objc2::rc::Retained<objc2::runtime::ProtocolObject<dyn objc2_foundation::NSObjectProtocol>>,
);

#[cfg(not(target_os = "macos"))]
pub struct NotificationWatch;

/// 窓が全画面から**抜け終わった**ら `on_exit` を呼ぶ（#59）。
///
/// tao はこの遷移を外へ出さない。delegate は 4 つとも実装しているのに、利用者へ届くのは
/// `Resized` / `Moved` だけで、アニメーション中に何度も来る同じイベントと区別が付かない
/// （tao 0.35 `macos/window_delegate.rs`）。そこで AppKit の通知を直接購読する。
/// 通知は delegate の呼び出しとは独立に出るので、**tao の delegate を奪わずに済む**
/// （奪うと `CloseRequested` も `Resized` も死ぬ）。
///
/// `queue` に `None` を渡すので、ブロックは通知を出したスレッド＝メインスレッドで
/// 同期に走る。イベントループのクロージャへ渡す手段（`EventLoopProxy`）はスレッド跨ぎで
/// 安全なので、`on_exit` の中でそれを撃てばよい。
#[cfg(target_os = "macos")]
pub fn watch_exit_fullscreen<F: Fn() + 'static>(
    window: &tao::window::Window,
    on_exit: F,
) -> Option<NotificationWatch> {
    use objc2_app_kit::{NSWindow, NSWindowDidExitFullScreenNotification};
    use objc2_foundation::NSNotificationCenter;
    use tao::platform::macos::WindowExtMacOS;

    let ptr = window.ns_window() as *mut NSWindow;
    // 不変条件は [`is_window_fullscreen`] と同じ。借りている `window` が生きている間は
    // このポインタも有効（tao の契約）で、メインスレッドから呼ばれる。
    let ns_window = unsafe { ptr.as_ref() }?;

    let block = block2::RcBlock::new(move |_notification: std::ptr::NonNull<_>| {
        on_exit();
    });
    // 監視対象をこの窓に絞る（`object:` に窓を渡す）。絞らないと、将来窓が増えたときに
    // 他の窓の遷移でも起こされる。
    let token = unsafe {
        NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
            Some(NSWindowDidExitFullScreenNotification),
            Some(ns_window),
            None,
            &block,
        )
    };
    Some(NotificationWatch(token))
}

/// 購読しない。[`is_window_fullscreen`] が常に false を返すので、そもそも待ちに入らない。
#[cfg(not(target_os = "macos"))]
pub fn watch_exit_fullscreen<F: Fn() + 'static>(
    _window: &tao::window::Window,
    _on_exit: F,
) -> Option<NotificationWatch> {
    None
}

/// アプリが前面に出たら `on_active` を呼ぶ（#49）。
///
/// 隠した窓へ戻る道。窓を閉じてもプロセスが生き残るようになったので、Dock アイコンを
/// クリックしたときに窓が戻らないと、**窓を失ったように見える**。
///
/// 本来の口は `applicationShouldHandleReopen:hasVisibleWindows:` だが、あれは
/// `NSApplicationDelegate` のメソッドで、delegate は tao が持っている（奪うと
/// `CloseRequested` も `Resized` も死ぬ）。通知は delegate と独立に出るので、
/// [`watch_exit_fullscreen`] と同じ形でこちらを購読する。
///
/// 前面に出る理由は Dock クリックだけではない（⌘Tab、転送してきた md が撃つ
/// `activate`）。**窓が既に出ているかは呼ばれた側で見ること。**
#[cfg(target_os = "macos")]
pub fn watch_app_active<F: Fn() + 'static>(on_active: F) -> Option<NotificationWatch> {
    use objc2_app_kit::NSApplicationDidBecomeActiveNotification;
    use objc2_foundation::NSNotificationCenter;

    let block = block2::RcBlock::new(move |_notification: std::ptr::NonNull<_>| {
        on_active();
    });
    // `object:` は None。送り主は NSApp ただ 1 つなので絞る意味が無い。
    let token = unsafe {
        NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
            Some(NSApplicationDidBecomeActiveNotification),
            None,
            None,
            &block,
        )
    };
    Some(NotificationWatch(token))
}

/// 購読しない。隠す経路が macOS 専用（Dock も ⌘Tab も無い）なので、戻す道も要らない。
#[cfg(not(target_os = "macos"))]
pub fn watch_app_active<F: Fn() + 'static>(_on_active: F) -> Option<NotificationWatch> {
    None
}

#[cfg(target_os = "macos")]
pub fn get_frontmost_pid() -> Option<i32> {
    use objc2_app_kit::NSWorkspace;
    let workspace = NSWorkspace::sharedWorkspace();
    let app = workspace.frontmostApplication()?;
    Some(app.processIdentifier())
}

/// ウィンドウを閉じたとき、フォーカスを起動元（md を起動したターミナルなど）へ戻す。
///
/// macOS 14 で「協調的アクティベーション」が導入され、旧来の
/// `activateWithOptions(ActivateIgnoringOtherApps)` は no-op になった。加えて、
/// 非アクティブなアプリが他アプリのフォーカスを奪うこと自体が禁じられている。
/// そのため以下の手順を踏む（いずれも macOS 14+ の非 deprecated API）:
///
/// 1. md 自身がアクティブなときだけ処理する。非アクティブなら、ユーザーは既に別の
///    アプリを操作しているので、そのフォーカスを横取りしてはいけない。
/// 2. `yieldActivationToApplication:` で「起動元にフォーカスを譲る」許可を出す。
///    これ自体は何も動かさない“許可”で、無いとシステムに次の要求を拒否されうる。
/// 3. `activateFromApplication:options:` で起動元を実際に前面化する。
#[cfg(target_os = "macos")]
pub fn activate_pid(pid: i32) {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSApplicationActivationOptions, NSRunningApplication};

    // NSApplication はメインスレッド専用。閉じる処理はイベントループ（メインスレッド）
    // から呼ばれるので通常は Some。念のため、そうでなければ何もしない。
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let app = NSApplication::sharedApplication(mtm);

    // 自分がフォーカスを持っていないなら手を出さない（持っていないものは渡せないし、
    // ユーザーが今見ている別アプリを奪う事故になる）。
    if !app.isActive() {
        return;
    }

    // 戻し先が既に終了していれば諦める。
    let Some(target) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid) else {
        return;
    };

    let current = NSRunningApplication::currentApplication();
    app.yieldActivationToApplication(&target);
    target.activateFromApplication_options(&current, NSApplicationActivationOptions(0));
}

/// 転送（#31）の送り側から、既存の窓を持つ受け側を前面化する。
///
/// [`activate_pid`] の裏返しだが、2 つ違う。
///
/// 1. **自分がアクティブかを見ない。** 送り側は端末から起動された短命のプロセスで、
///    アクティブになったことが一度も無い。`spike/activation` の実測では、それでも
///    `activateFromApplication:` は通った（「持っていないものは譲れない」は外れ）。
/// 2. **`yieldActivationToApplication:` を撃たない。** 有無で結果が変わらなかった。
///
/// `NSApplication::sharedApplication` を**呼ばないこと**。送り側は run loop を回さない
/// ので、ここで AppKit を起こすとその初期化コストを転送のたびに払う。`NSApp` 越しの
/// `activate()` は実測で 3 回とも不発だったので、呼ぶ利点も無い。
///
/// 前面化の責任はここ 1 箇所だけが持つ。受け側（`AppEvent::Open` の腕）は隠れた窓を
/// 持ち上げるだけで、アプリを前へ出すことはしない。
#[cfg(target_os = "macos")]
pub fn activate_other(pid: i32) {
    use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication};

    let Some(target) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid) else {
        return;
    };
    let me = NSRunningApplication::currentApplication();
    target.activateFromApplication_options(&me, NSApplicationActivationOptions(0));
}

#[cfg(not(target_os = "macos"))]
pub fn activate_other(_pid: i32) {}

/// Dock と ⌘Tab に出るアイコンを差し込む。
///
/// md は `.app` を作らない（bundle.rs 参照）ので `Info.plist` の `CFBundleIconFile`
/// が使えず、素の実行ファイルのままだと Dock に汎用の実行ファイルアイコンが出る。
/// 起動時に NSApplication へ画像を渡せば、バンドル無しでもそこだけ差し替わる。
/// 512px 1 枚で足りる: Dock の最大表示は 128@2x で、⌘Tab や強制終了ダイアログも
/// これ以上は要求しない。
#[cfg(target_os = "macos")]
pub fn set_dock_icon() {
    use objc2::{AnyThread, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;

    const ICON_PNG: &[u8] = include_bytes!("assets/icon.png");

    let mtm = MainThreadMarker::new().expect("must be on main thread");
    let data = NSData::with_bytes(ICON_PNG);
    // 埋め込みの PNG が壊れていることは無いが、デコードに失敗しても起動は止めない
    // （アイコンが既定のままになるだけ）。
    let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) else {
        return;
    };
    // SAFETY: image は有効な NSImage で、呼び出しはメインスレッド（mtm で保証）。
    unsafe { NSApplication::sharedApplication(mtm).setApplicationIconImage(Some(&image)) };
}

/// [`MenuAction`] が抱えるもの。`Box<dyn Fn()>` を objc のオブジェクトに載せるための箱。
#[cfg(target_os = "macos")]
struct MenuActionIvars {
    on_action: Box<dyn Fn()>,
}

// `define_class!` の `#[thread_kind = MainThreadOnly]` が名前で解決するので、
// モジュールの位置で入れておく必要がある（関数の中の use では届かない）。
#[cfg(target_os = "macos")]
use objc2::{DefinedClass, MainThreadOnly};

#[cfg(target_os = "macos")]
objc2::define_class!(
    // SAFETY:
    // - 親の NSObject はサブクラス化に条件を課さない。
    // - Drop を実装しないので dealloc の生成も要らない。
    #[unsafe(super(objc2::runtime::NSObject))]
    // メニューの action はメインスレッドからしか来ない。
    #[thread_kind = MainThreadOnly]
    #[ivars = MenuActionIvars]
    struct MenuAction;

    impl MenuAction {
        #[unsafe(method(mdInvoke:))]
        fn invoke(&self, _sender: Option<&objc2::runtime::AnyObject>) {
            (self.ivars().on_action)();
        }
    }
);

#[cfg(target_os = "macos")]
impl MenuAction {
    fn new(
        mtm: objc2::MainThreadMarker,
        on_action: Box<dyn Fn()>,
    ) -> objc2::rc::Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(MenuActionIvars { on_action });
        unsafe { objc2::msg_send![super(this), init] }
    }
}

/// [`setup_menu`] が返す札。**メニュー項目は target を retain しない**ので、宛先は
/// 呼び出し側が持ち続ける。落とすと ⌘Q が解放済みのオブジェクトへ飛ぶ。
#[cfg(target_os = "macos")]
pub struct MenuTargets(#[allow(dead_code)] Vec<objc2::rc::Retained<MenuAction>>);

/// メニューバーを組む。`on_quit` はメニューの Quit（⌘Q）から呼ばれる。
///
/// **Quit は `terminate:` でも `performClose:` でもない。**
/// - `terminate:` は tao の `CloseRequested` を経由せずプロセスを即終了するので、
///   一時ファイルの後始末も、全画面から抜ける待ち（#59）も走らない
/// - `performClose:` は × ボタンと同じ穴で、#49 でそこは「隠す」になった。
///   ⌘Q まで隠す側へ行くと、**終了する手段がメニューから消える**
///
/// そこで呼び出し側のクロージャを持つ宛先を自前で立てて、イベントループへ流す。
/// 終了の手順は閉じる要求と 1 本に合流するので、全画面の始末も後始末も共通になる。
#[cfg(target_os = "macos")]
pub fn setup_menu<F: Fn() + 'static>(on_quit: F) -> MenuTargets {
    use objc2::sel;
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSMenu, NSMenuItem};
    use objc2_foundation::ns_string;

    let mtm = MainThreadMarker::new().expect("must be on main thread");
    let quit_target = MenuAction::new(mtm, Box::new(on_quit));

    let menubar = NSMenu::new(mtm);

    let app_item = NSMenuItem::new(mtm);
    let app_menu = NSMenu::new(mtm);
    unsafe {
        let quit = NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            ns_string!("Quit"),
            Some(sel!(mdInvoke:)),
            ns_string!("q"),
        );
        // target を明示する。nil のままだとレスポンダチェーンを辿るが、`mdInvoke:` に
        // 応えるのはこの宛先だけなので、誰も拾わず項目が灰色になる。
        quit.setTarget(Some(&quit_target));
        app_menu.addItem(&quit);
        app_item.setSubmenu(Some(&app_menu));
        menubar.addItem(&app_item);
    }

    let edit_item = NSMenuItem::new(mtm);
    let edit_menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), ns_string!("Edit"));
    let make = |title: &objc2_foundation::NSString,
                sel_: objc2::runtime::Sel,
                key: &objc2_foundation::NSString| {
        unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                title,
                Some(sel_),
                key,
            )
        }
    };

    edit_menu.addItem(&make(ns_string!("Undo"),       sel!(undo:),      ns_string!("z")));
    let redo = make(ns_string!("Redo"), sel!(redo:), ns_string!("z"));
    redo.setKeyEquivalentModifierMask(
        objc2_app_kit::NSEventModifierFlags::Command
            | objc2_app_kit::NSEventModifierFlags::Shift,
    );
    edit_menu.addItem(&redo);
    edit_menu.addItem(&NSMenuItem::separatorItem(mtm));
    edit_menu.addItem(&make(ns_string!("Cut"),        sel!(cut:),       ns_string!("x")));
    edit_menu.addItem(&make(ns_string!("Copy"),       sel!(copy:),      ns_string!("c")));
    edit_menu.addItem(&make(ns_string!("Paste"),      sel!(paste:),     ns_string!("v")));
    edit_menu.addItem(&NSMenuItem::separatorItem(mtm));
    edit_menu.addItem(&make(ns_string!("Select All"), sel!(selectAll:), ns_string!("a")));

    edit_item.setSubmenu(Some(&edit_menu));
    menubar.addItem(&edit_item);

    // 緑ボタンのフルスクリーンを ⌃⌘F（macOS 標準）でも切り替えられるようにする。
    // AppKit は View メニューがあれば "Enter Full Screen" を自前で足してくれるが、
    // setMainMenu でメニューバーを丸ごと差し替えているぶん、その項目も消えている。
    // toggleFullScreen: は target=nil のままでよい（レスポンダ連鎖で NSWindow が受ける）。
    let view_item = NSMenuItem::new(mtm);
    let view_menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), ns_string!("View"));
    let full = make(
        ns_string!("Enter Full Screen"),
        sel!(toggleFullScreen:),
        ns_string!("f"),
    );
    full.setKeyEquivalentModifierMask(
        objc2_app_kit::NSEventModifierFlags::Command
            | objc2_app_kit::NSEventModifierFlags::Control,
    );
    view_menu.addItem(&full);
    view_item.setSubmenu(Some(&view_menu));
    menubar.addItem(&view_item);

    let app = NSApplication::sharedApplication(mtm);
    app.setMainMenu(Some(&menubar));

    MenuTargets(vec![quit_target])
}
