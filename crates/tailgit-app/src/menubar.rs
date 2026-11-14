//! Native macOS menu bar. Registering the Window submenu with AppKit makes the
//! system populate it (Zoom, Fill, Center, Move & Resize, Tile, window list...).

#[cfg(target_os = "macos")]
pub fn install() {
    use muda::{Menu, PredefinedMenuItem as P, Submenu};

    let menu = Menu::new();

    let app = Submenu::new("TailGIT", true);
    let _ = app.append_items(&[
        &P::about(None, None),
        &P::separator(),
        &P::services(None),
        &P::separator(),
        &P::hide(None),
        &P::hide_others(None),
        &P::show_all(None),
        &P::separator(),
        &P::quit(None),
    ]);

    let edit = Submenu::new("Edit", true);
    let _ = edit.append_items(&[
        &P::undo(None),
        &P::redo(None),
        &P::separator(),
        &P::cut(None),
        &P::copy(None),
        &P::paste(None),
        &P::select_all(None),
    ]);

    let window = Submenu::new("Window", true);
    let _ = window.append_items(&[
        &P::minimize(None),
        &P::maximize(None),
        &P::separator(),
        &P::bring_all_to_front(None),
    ]);

    let help = Submenu::new("Help", true);

    let _ = menu.append_items(&[&app, &edit, &window, &help]);
    menu.init_for_nsapp();
    window.set_as_windows_menu_for_nsapp();
    help.set_as_help_menu_for_nsapp();

    // The menu bar must outlive the app.
    std::mem::forget((menu, app, edit, window, help));
}

#[cfg(not(target_os = "macos"))]
pub fn install() {}
