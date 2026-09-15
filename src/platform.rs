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

/// webview の「ページ範囲外」の色を塗り直す。行き過ぎスクロールの跳ね返り領域が
/// これにあたる。
///
/// wry の `WebView::set_background_color` は使えない。macOS 側の実装が丸ごと
/// `transparent` feature の中にあり、その feature を有効にしていないので、呼んでも
/// 何もせず `Ok(())` が返るだけになる。起動時にビルダーへ渡す分だけは feature の
/// 外（`setUnderPageBackgroundColor`）なので効いている。後から変えるには、こうして
/// 直接叩くしかない。
///
/// `setUnderPageBackgroundColor` は macOS 12 以降にしかないので、応答するか確かめて
/// から呼ぶ。11 以下では跳ね返りの色が起動時のまま残る。
#[cfg(target_os = "macos")]
pub fn set_webview_under_page_color(webview: &wry::WebView, (r, g, b, a): (u8, u8, u8, u8)) {
    use objc2::{sel, runtime::NSObjectProtocol};
    use objc2_app_kit::NSColor;
    use wry::WebViewExtMacOS;

    let view = webview.webview();
    if !view.respondsToSelector(sel!(setUnderPageBackgroundColor:)) {
        return;
    }
    let color = NSColor::colorWithSRGBRed_green_blue_alpha(
        r as f64 / 255.0,
        g as f64 / 255.0,
        b as f64 / 255.0,
        a as f64 / 255.0,
    );
    unsafe { view.setUnderPageBackgroundColor(Some(&color)) };
}

#[cfg(not(target_os = "macos"))]
pub fn set_webview_under_page_color(_webview: &wry::WebView, _color: (u8, u8, u8, u8)) {}

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

#[cfg(target_os = "macos")]
pub fn setup_menu() {
    use objc2::sel;
    use objc2::{MainThreadMarker, MainThreadOnly};
    use objc2_app_kit::{NSApplication, NSMenu, NSMenuItem};
    use objc2_foundation::ns_string;

    let mtm = MainThreadMarker::new().expect("must be on main thread");

    let menubar = NSMenu::new(mtm);

    let app_item = NSMenuItem::new(mtm);
    let app_menu = NSMenu::new(mtm);
    unsafe {
        // terminate: は tao の CloseRequested を経由せずプロセスを即終了するため、
        // 閉じたときのフォーカス戻し（activate_pid）が走らない。performClose: にすると
        // ウィンドウ閉じ → windowShouldClose: → CloseRequested に乗り、×ボタンと同じ
        // 経路を通る。単一ウィンドウなので「閉じる＝終了」で体験は変わらない。
        let quit = NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            ns_string!("Quit"),
            Some(sel!(performClose:)),
            ns_string!("q"),
        );
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
}
