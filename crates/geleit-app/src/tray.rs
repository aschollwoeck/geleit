//! System tray icon (NOTIF-4) — a persistent presence so GeleitMail keeps running, and checking mail,
//! after you close the window.
//!
//! Left-clicking the icon (or the **Show** menu item) brings the window back; **Quit** is the only
//! thing that actually exits. The icon's tooltip mirrors the unread count, updated from the same one
//! chokepoint that sets the window title ([`crate::ipc::set_badge`]), so the two never disagree.
//!
//! **Close-to-tray is conditional.** Making the window's close button *hide* rather than quit is only
//! safe when the desktop actually shows a tray icon — otherwise a hidden window has nothing to bring it
//! back. So it's enabled only when a `StatusNotifierWatcher` is present ([`geleit_platform::tray`]); on
//! a bare desktop (vanilla GNOME) closing quits as before. And the tray never blocks startup: if it
//! can't be built at all, the window still runs — just without the icon.

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Manager, WindowEvent};

/// The tray's id, so [`crate::ipc::set_badge`] can find it again by `app.tray_by_id` to refresh the
/// tooltip. One tray, one id.
pub(crate) const TRAY_ID: &str = "main";

/// Build the tray icon and, when the desktop will actually show it, make the window's close button
/// hide to the tray instead of quitting. Called once, in the Tauri `setup` hook, after the main
/// window exists. Never returns `Err` for a tray problem — a missing tray must not stop the app.
pub(crate) fn setup(app: &tauri::AppHandle) {
    if let Err(e) = build_tray(app) {
        // No icon (no D-Bus, or the host refused it): the window still works, close still quits. Log
        // and carry on rather than take the whole app down for a cosmetic affordance.
        eprintln!("tray: icon unavailable, continuing without it ({e})");
        return;
    }
    // The icon exists — but only hide-on-close if something will paint it, or the window would vanish
    // with no way back.
    if geleit_platform::tray::host_present() {
        enable_close_to_tray(app);
    }
}

/// Create the tray icon with its menu and click behaviour. Errors bubble up to [`setup`], which treats
/// them as "no tray".
fn build_tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Show GeleitMail", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit GeleitMail", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("GeleitMail")
        .menu(&menu)
        // Left-click reopens the window (handled below); the menu is the right-click affordance, so it
        // must not also pop on left-click or the reopen click would open the menu instead.
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        });
    // The app icon doubles as the tray icon. If it isn't embedded, the tray still works — it just
    // shows the platform's default icon.
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

/// Intercept the window's close so it hides to the tray instead of quitting — mail keeps arriving in
/// the background, and the icon is how you get the window back. Only wired when a tray host is present.
fn enable_close_to_tray(app: &tauri::AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let handle = app.clone();
        win.on_window_event(move |event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                if let Some(w) = handle.get_webview_window("main") {
                    let _ = w.hide();
                }
            }
        });
    }
}

/// Bring the main window back to the foreground: show it (it may be hidden to the tray), un-minimise,
/// and focus it. Best-effort — a missing window (mid-shutdown) is simply ignored.
fn show_main(app: &tauri::AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.show();
        let _ = win.unminimize();
        let _ = win.set_focus();
    }
}
