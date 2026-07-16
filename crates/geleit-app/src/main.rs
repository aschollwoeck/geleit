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
mod idle;
mod ipc;
mod mailproto;
mod notify;
mod schedule;
mod scheduler;

use geleit_platform::os_secret::OsSecretStore;
use ipc::AppState;
use std::sync::Arc;
use tauri::{WebviewUrl, WebviewWindowBuilder};

/// Hand a URL to the user's real browser. Deliberately a subprocess rather than a Tauri plugin: no
/// new capability to grant, and — the point — **no HTTP client in the app**. GeleitMail never fetches
/// the page; the browser does. Only ever called with an `http(s)`/`mailto` URL already parsed by
/// `url::Url` (so it is a well-formed URL, not arbitrary text).
fn open_externally(url: &str) {
    #[cfg(not(target_os = "windows"))]
    {
        #[cfg(target_os = "linux")]
        let cmd = "xdg-open";
        #[cfg(target_os = "macos")]
        let cmd = "open";
        let _ = std::process::Command::new(cmd).arg(url).spawn();
    }
    #[cfg(target_os = "windows")]
    {
        // NOT `cmd /C start`: `start` re-parses its line with cmd metacharacter rules (`&`, `|`,
        // `^`, …) that std's argument quoting does not neutralize for cmd (the BatBadBut class,
        // CVE-2024-24576), and the URL comes from an attacker-controlled mail link. `explorer.exe`
        // takes the URL as a single argv element with no shell re-parsing. Refuse anything with a
        // cmd/shell metacharacter as a second line of defence. (Windows isn't a shipping target yet
        // — S8.4 — so this path is untested here; revisit with a real ShellExecuteW when it ships.)
        if url.contains(['&', '|', '^', '<', '>', '"', '%', '\n', '\r']) {
            return;
        }
        let _ = std::process::Command::new("explorer.exe").arg(url).spawn();
    }
}

/// The exact hosts the app's own webview serves from on Windows, where custom schemes are exposed as
/// `http://<scheme>.localhost`. On Linux/macOS these are real custom schemes and never reach the
/// http branch. An arbitrary `*.localhost` is **not** ours — a mail link to `http://ipc.localhost/`
/// must not be treated as in-app navigation.
const OWN_HTTP_HOSTS: [&str; 3] = ["tauri.localhost", "ipc.localhost", "mail.localhost"];

/// What a navigation attempt should do. Deciding this is **pure**, so it can be unit-tested without
/// launching anything — see [`navigation_action`]. (It used to be fused with the side effect, and the
/// tests really did spawn a browser and a mail client for every fixture URL. They don't now.)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NavAction {
    /// Our own origin — let the webview load it.
    Load,
    /// A real link in a message — hand it to the system browser and cancel the in-app navigation.
    OpenExternally,
    /// Anything else — refuse outright.
    Refuse,
}

/// Decide what a navigation attempt is allowed to do. **Pure** — no process is spawned here.
///
/// The app itself must **never** navigate to anything but its own origins: the only page it may show
/// is its own UI. A link in a message is a request to leave, and leaving means the *system browser*,
/// not this window — otherwise a crafted `http://…localhost` link could load the app's own origin
/// (with its IPC bridge) in-window, or a message could replace GeleitMail with a look-alike page.
fn navigation_action(url: &url::Url) -> NavAction {
    match url.scheme() {
        // Our own UI and the mail origin (custom schemes on Linux/macOS). NOT data:/blob:/about —
        // none is needed, and a top-level `data:text/html,<script>` navigation would run under an
        // opaque origin, so they stay off the allowlist as defence in depth.
        "tauri" | "mail" => NavAction::Load,
        // On Windows the same origins appear as http://<scheme>.localhost — allow ONLY those exact
        // hosts, never an arbitrary `*.localhost`.
        "http" | "https" if url.host_str().is_some_and(|h| OWN_HTTP_HOSTS.contains(&h)) => {
            NavAction::Load
        }
        "http" | "https" | "mailto" => NavAction::OpenExternally,
        // anything else (javascript:, file:, data:, blob:, …) is simply refused
        _ => NavAction::Refuse,
    }
}

/// The webview's navigation hook: apply [`navigation_action`] and perform its side effect. Returns
/// `true` to let the webview proceed (only for our own origins), `false` to cancel.
fn allow_navigation(url: &url::Url) -> bool {
    match navigation_action(url) {
        NavAction::Load => true,
        NavAction::OpenExternally => {
            open_externally(url.as_str()); // a real link → the system browser
            false
        }
        NavAction::Refuse => false,
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
        ipc::list_all_messages,
        ipc::open_message,
        ipc::set_star,
        ipc::set_read,
        ipc::set_unread,
        ipc::move_to_role,
        ipc::move_to_folder,
        ipc::empty_trash,
        ipc::delete_forever,
        ipc::create_folder,
        ipc::rename_folder,
        ipc::delete_folder,
        ipc::refresh,
        ipc::compose_draft,
        ipc::send_message,
        ipc::suggest_addresses,
        ipc::save_draft,
        ipc::list_drafts,
        ipc::load_draft,
        ipc::refresh_drafts,
        ipc::resume_server_draft,
        ipc::delete_draft,
        ipc::purge_server_drafts,
        ipc::pick_files,
        ipc::save_eml,
        ipc::open_eml_file,
        ipc::save_attachment,
        ipc::add_account,
        ipc::search,
        ipc::search_all,
        ipc::set_theme,
        ipc::remove_account,
        ipc::get_bool_setting,
        ipc::set_bool_setting,
        ipc::get_setting,
        ipc::set_setting,
        ipc::update_badge,
        ipc::outbox_status,
        ipc::list_outbox,
        ipc::retry_outbox,
        ipc::discard_outbox,
        ipc::edit_outbox,
        ipc::get_signature,
        ipc::set_signature,
        ipc::theme,
        ipc::dev_open_message,
        ipc::dev_load_images,
        ipc::dev_compose,
        ipc::dev_unified,
        ipc::dev_setup,
        ipc::dev_settings,
        ipc::dev_search,
        ipc::dev_trash,
        ipc::dev_compose_to,
        ipc::dev_drafts,
        ipc::dev_resume,
        ipc::dev_select,
        ipc::dev_folder,
    ]);
    #[cfg(not(debug_assertions))]
    let builder = builder.invoke_handler(tauri::generate_handler![
        ipc::list_accounts,
        ipc::list_folders,
        ipc::list_messages,
        ipc::list_all_messages,
        ipc::open_message,
        ipc::set_star,
        ipc::set_read,
        ipc::set_unread,
        ipc::move_to_role,
        ipc::move_to_folder,
        ipc::empty_trash,
        ipc::delete_forever,
        ipc::create_folder,
        ipc::rename_folder,
        ipc::delete_folder,
        ipc::refresh,
        ipc::compose_draft,
        ipc::send_message,
        ipc::suggest_addresses,
        ipc::save_draft,
        ipc::list_drafts,
        ipc::load_draft,
        ipc::refresh_drafts,
        ipc::resume_server_draft,
        ipc::delete_draft,
        ipc::purge_server_drafts,
        ipc::pick_files,
        ipc::save_eml,
        ipc::open_eml_file,
        ipc::save_attachment,
        ipc::add_account,
        ipc::search,
        ipc::search_all,
        ipc::set_theme,
        ipc::remove_account,
        ipc::get_bool_setting,
        ipc::set_bool_setting,
        ipc::get_setting,
        ipc::set_setting,
        ipc::update_badge,
        ipc::outbox_status,
        ipc::list_outbox,
        ipc::retry_outbox,
        ipc::discard_outbox,
        ipc::edit_outbox,
        ipc::get_signature,
        ipc::set_signature,
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
                // Perf-budget harness (S9.8): when GELEIT_PERF=1, print a marker the moment the page
                // loads, so the CI cold-start check can time exec→first-paint without a window
                // manager (works headless under xvfb). No effect otherwise.
                .on_page_load(|_win, _payload| {
                    if std::env::var_os("GELEIT_PERF").is_some() {
                        println!("GELEIT_READY");
                    }
                })
                .build()?;
            // Mail arrives on its own from here — the host polls, so it keeps working while the UI
            // sits idle (a webview throttles timers in a hidden window).
            scheduler::spawn(app.handle().clone());
            idle::spawn(app.handle().clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("GeleitMail could not start its window.");
}

#[cfg(test)]
mod tests {
    use super::{navigation_action, NavAction};

    /// Decide only — deliberately NOT `allow_navigation`, which would really spawn a browser / mail
    /// client for every external fixture URL below. (It used to, on every `cargo test`.)
    fn nav(u: &str) -> NavAction {
        navigation_action(&url::Url::parse(u).unwrap())
    }

    #[test]
    fn our_own_origins_may_load() {
        assert_eq!(nav("tauri://localhost/index.html"), NavAction::Load);
        assert_eq!(nav("mail://localhost/42"), NavAction::Load);
        assert_eq!(nav("http://tauri.localhost/index.html"), NavAction::Load); // Windows
        assert_eq!(nav("http://mail.localhost/42"), NavAction::Load); // Windows
        assert_eq!(nav("http://ipc.localhost/cmd"), NavAction::Load); // Windows IPC
    }

    /// A link in a message must NOT navigate the app — otherwise a message could replace GeleitMail
    /// with a look-alike page still wearing its window frame. It goes to the system browser instead.
    #[test]
    fn a_remote_link_never_navigates_the_app() {
        assert_eq!(nav("https://example.com/phish"), NavAction::OpenExternally);
        assert_eq!(nav("http://example.com/"), NavAction::OpenExternally);
        assert_eq!(nav("mailto:someone@example.com"), NavAction::OpenExternally);
    }

    /// `.localhost` must not be a blanket loophole. The S9.2 review flagged this: a mail link to the
    /// app's OWN framework origins was allowed for *any* `*.localhost`, so `http://evil.localhost/`
    /// (and, worse, `http://ipc.localhost` reached from a lookalike) would navigate in-window. Only
    /// the exact framework hosts are ours; every other loopback host goes to the browser.
    #[test]
    fn an_arbitrary_localhost_host_is_not_treated_as_ours() {
        assert_eq!(nav("http://evil.localhost/"), NavAction::OpenExternally);
        assert_eq!(
            nav("http://notmail.localhost/42"),
            NavAction::OpenExternally
        );
        assert_eq!(
            nav("https://evil-localhost.example/"),
            NavAction::OpenExternally
        );
        assert_eq!(
            nav("https://tauri.localhost.evil.example/"),
            NavAction::OpenExternally
        );
    }

    /// These are refused outright — not even handed to the browser.
    #[test]
    fn dangerous_schemes_are_refused_outright() {
        assert_eq!(nav("file:///etc/passwd"), NavAction::Refuse);
        assert_eq!(nav("javascript:alert(1)"), NavAction::Refuse);
        // data:/blob: are NOT navigable — a top-level data:text/html would run under an opaque origin
        assert_eq!(
            nav("data:text/html,<script>alert(1)</script>"),
            NavAction::Refuse
        );
        assert_eq!(nav("blob:https://example.com/uuid"), NavAction::Refuse);
        assert_eq!(nav("about:blank"), NavAction::Refuse);
    }
}
