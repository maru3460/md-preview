use std::path::Path;

/// 右クリックメニュー由来のクリップボード書き込み（絶対パス）。
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

    let app = NSApplication::sharedApplication(mtm);
    app.setMainMenu(Some(&menubar));
}
