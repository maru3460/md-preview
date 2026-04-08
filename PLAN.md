# Plan: macOS メニューバー追加 (Cmd+C / Cmd+A 対応)

## Context
wry の WebView は macOS のレスポンダーチェーンを使うため、アプリに NSMenu が存在しないと Cmd+C (コピー) や Cmd+A (全選択) などの標準編集ショートカットが機能しない。tao 0.35 はメニューモジュールを持たないため、既存の objc2 / objc2-app-kit 依存を使って直接 NSMenu を組み立てる。

## 実装方針

### 1. Cargo.toml の変更
`objc2-app-kit` のフィーチャーリストに `"NSApplication"`, `"NSMenu"`, `"NSMenuItem"` を追加する。

**ファイル:** `Cargo.toml` (line 18)

変更前:
```toml
objc2-app-kit = { version = "0.3", features = ["NSWorkspace", "NSRunningApplication"] }
```
変更後:
```toml
objc2-app-kit = { version = "0.3", features = [
    "NSWorkspace", "NSRunningApplication",
    "NSApplication", "NSMenu", "NSMenuItem", "NSEvent",
] }
```

### 2. `setup_menu()` 関数を追加
`src/main.rs` に `#[cfg(target_os = "macos")]` ガードつきで以下の関数を追加する。

```rust
#[cfg(target_os = "macos")]
fn setup_menu() {
    use objc2::sel;
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSMenu, NSMenuItem};
    use objc2_foundation::ns_string;

    let mtm = MainThreadMarker::new().expect("must be on main thread");

    // アプリ全体のメニューバー
    let menubar = NSMenu::new(mtm);

    // ── App メニュー（先頭の太字メニュー） ──
    let app_item = unsafe { NSMenuItem::new(mtm) };
    let app_menu = NSMenu::new(mtm);
    unsafe {
        let quit = NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(),
            ns_string!("Quit"),
            Some(sel!(terminate:)),
            ns_string!("q"),
        );
        app_menu.addItem(&quit);
        app_item.setSubmenu(Some(&app_menu));
        menubar.addItem(&app_item);
    }

    // ── Edit メニュー ──
    let edit_item = unsafe { NSMenuItem::new(mtm) };
    let edit_menu = NSMenu::initWithTitle(NSMenu::alloc(), ns_string!("Edit"));
    unsafe {
        let make = |title: &objc2_foundation::NSString, sel_: objc2::runtime::Sel, key: &objc2_foundation::NSString| {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(),
                title,
                Some(sel_),
                key,
            )
        };

        edit_menu.addItem(&make(ns_string!("Undo"),      sel!(undo:),      ns_string!("z")));
        let redo = make(ns_string!("Redo"), sel!(redo:), ns_string!("z"));
        redo.setKeyEquivalentModifierMask(
            objc2_app_kit::NSEventModifierFlags::Command
            | objc2_app_kit::NSEventModifierFlags::Shift,
        );
        edit_menu.addItem(&redo);
        edit_menu.addItem(&NSMenuItem::separatorItem(mtm));
        edit_menu.addItem(&make(ns_string!("Cut"),       sel!(cut:),       ns_string!("x")));
        edit_menu.addItem(&make(ns_string!("Copy"),      sel!(copy:),      ns_string!("c")));
        edit_menu.addItem(&make(ns_string!("Paste"),     sel!(paste:),     ns_string!("v")));
        edit_menu.addItem(&NSMenuItem::separatorItem(mtm));
        edit_menu.addItem(&make(ns_string!("Select All"),sel!(selectAll:), ns_string!("a")));

        edit_item.setSubmenu(Some(&edit_menu));
        menubar.addItem(&edit_item);
    }

    let app = NSApplication::sharedApplication(mtm);
    app.setMainMenu(Some(&menubar));
}
```

### 3. `main()` 内で呼び出し
`event_loop.run(...)` の直前に追加する (line 246 付近):

```rust
#[cfg(target_os = "macos")]
setup_menu();

event_loop.run(move |event, ...
```

## 変更ファイル
- `Cargo.toml` — objc2-app-kit フィーチャー追加
- `src/main.rs` — `setup_menu()` 関数追加 + 呼び出し

## 検証
```
cargo build
./target/debug/md test.md
```
- Cmd+A でテキスト全選択できることを確認
- Cmd+C でコピーできることを確認
- Cmd+W でウィンドウが閉じることを確認 (既存動作の回帰なし)
