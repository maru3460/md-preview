#[cfg(target_os = "macos")]
pub fn get_frontmost_pid() -> Option<libc::pid_t> {
    use objc2_app_kit::NSWorkspace;
    let workspace = NSWorkspace::sharedWorkspace();
    let app = workspace.frontmostApplication()?;
    Some(app.processIdentifier())
}

#[cfg(target_os = "macos")]
pub fn activate_pid(pid: libc::pid_t) {
    use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication};
    if let Some(app) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid) {
        #[allow(deprecated)]
        app.activateWithOptions(NSApplicationActivationOptions::ActivateIgnoringOtherApps);
    }
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
        let quit = NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            ns_string!("Quit"),
            Some(sel!(terminate:)),
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
