//! GeleitMail — the Tauri shell (M9, ADR-0012).
//!
//! The window is the OS webview, and the UI inside it is `geleit-ui` (Leptos, Rust → WASM). This
//! binary owns three things: the window, the typed IPC seam to the engine ([`ipc`]), and the
//! `mail://` origin that a message's HTML is served from ([`mailproto`]).
//!
//! Why a webview at all, given constitution P4 once said the opposite: two attempts at native HTML
//! rendering failed on their merits (an embedded webview crashed on X11; a pure-Rust CPU renderer
//! could not render real mail). Rendering hostile mail HTML correctly *and* safely needs a real
//! browser engine. P4 was amended in M9 to say so honestly.
//!
//! **Mail never renders in this document.** It is confined to a sandboxed `<iframe>` on its own
//! origin, with no `allow-scripts` and no `allow-same-origin`.
//!
//! Runs alongside the Slint `geleit-app` until S9.7's teardown, so the shipped app keeps working
//! throughout the migration.
mod dto;
mod ipc;
mod mailproto;

use geleit_platform::os_secret::OsSecretStore;
use ipc::AppState;
use std::sync::Arc;
use tauri::{WebviewUrl, WebviewWindowBuilder};

/// Hand a URL to the user's real browser. Deliberately a subprocess rather than a Tauri plugin: no
/// new capability to grant, and — the point — **no HTTP client in the app**. GeleitMail never fetches
/// the page; the browser does.
fn open_externally(url: &str) {
    #[cfg(target_os = "linux")]
    let (cmd, args): (&str, &[&str]) = ("xdg-open", &[]);
    #[cfg(target_os = "macos")]
    let (cmd, args): (&str, &[&str]) = ("open", &[]);
    #[cfg(target_os = "windows")]
    let (cmd, args): (&str, &[&str]) = ("cmd", &["/C", "start", ""]);

    let _ = std::process::Command::new(cmd).args(args).arg(url).spawn();
}

/// Decide what a navigation attempt is allowed to do.
///
/// The app itself must **never** navigate: the only page it may show is its own. A link in a message
/// is a request to leave, and leaving means the *system browser*, not this window — otherwise a
/// message could replace the app with a look-alike page that still wears GeleitMail's frame.
///
/// Returns `true` to let the webview proceed (only ever for our own origins), `false` to cancel.
fn allow_navigation(url: &url::Url) -> bool {
    match url.scheme() {
        // our own UI and the mail origin (Windows serves custom schemes over http://<scheme>.localhost)
        "tauri" | "mail" | "about" | "blob" | "data" => true,
        "http" | "https" => {
            let host = url.host_str().unwrap_or_default();
            if host.ends_with(".localhost") || host == "localhost" {
                return true; // Tauri's own asset/IPC/mail origins on Windows
            }
            open_externally(url.as_str());
            false
        }
        "mailto" => {
            open_externally(url.as_str());
            false
        }
        // anything else (javascript:, file:, …) is simply refused
        _ => false,
    }
}

fn main() {
    // Same dev bridge as the Slint app: `GELEIT_DB` overrides the mailbox path.
    let db_path = std::env::var("GELEIT_DB").unwrap_or_else(|_| "geleit.db".to_owned());
    // The at-rest key + credentials live in the OS keychain (SEC-2/SEC-1, ADR-0008).
    let state = AppState::new(db_path, Arc::new(OsSecretStore::new()));

    let builder = tauri::Builder::default()
        .manage(state)
        // A message's HTML is served here, on its own origin — never `srcdoc` (which would inherit
        // the app's CSP and strip every message's styles). See `mailproto`.
        .register_uri_scheme_protocol("mail", mailproto::handle);

    // The dev-only screenshot seam exists only in debug builds — see `ipc::dev_open_message`.
    #[cfg(debug_assertions)]
    let builder = builder.invoke_handler(tauri::generate_handler![
        ipc::list_accounts,
        ipc::list_folders,
        ipc::list_messages,
        ipc::open_message,
        ipc::theme,
        ipc::dev_open_message,
        ipc::dev_load_images,
    ]);
    #[cfg(not(debug_assertions))]
    let builder = builder.invoke_handler(tauri::generate_handler![
        ipc::list_accounts,
        ipc::list_folders,
        ipc::list_messages,
        ipc::open_message,
        ipc::theme,
    ]);

    builder
        // The window is built here rather than declared in tauri.conf.json because the navigation
        // guard can only be attached at build time — and without it, a link in a message could
        // navigate the app itself.
        .setup(|app| {
            WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("GeleitMail")
                .inner_size(1200.0, 820.0)
                .min_inner_size(720.0, 480.0)
                // No cookie jar, no persistent cache: image loads the reader opts into (PRIV-2)
                // cannot be correlated across sessions.
                .incognito(true)
                .on_navigation(allow_navigation)
                .build()?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("GeleitMail could not start its window.");
}

#[cfg(test)]
mod tests {
    use super::allow_navigation;

    fn nav(u: &str) -> bool {
        allow_navigation(&url::Url::parse(u).unwrap())
    }

    #[test]
    fn our_own_origins_may_load() {
        assert!(nav("tauri://localhost/index.html"));
        assert!(nav("mail://localhost/42"));
        assert!(nav("http://tauri.localhost/index.html")); // Windows
        assert!(nav("http://mail.localhost/42")); // Windows
    }

    /// A link in a message must NOT navigate the app — otherwise a message could replace GeleitMail
    /// with a look-alike page still wearing its window frame. (These spawn a browser process in a
    /// real run; here we only assert the navigation is refused.)
    #[test]
    fn a_remote_link_never_navigates_the_app() {
        assert!(!nav("https://example.com/phish"));
        assert!(!nav("http://example.com/"));
        assert!(!nav("mailto:someone@example.com"));
    }

    /// `localhost` must not be a loophole for an arbitrary remote host that merely *ends* in it.
    #[test]
    fn a_lookalike_host_is_not_treated_as_ours() {
        assert!(!nav("https://evil-localhost.example/"));
        assert!(!nav("https://tauri.localhost.evil.example/"));
    }

    #[test]
    fn other_schemes_are_refused_outright() {
        assert!(!nav("file:///etc/passwd"));
        assert!(!nav("javascript:alert(1)"));
    }
}
