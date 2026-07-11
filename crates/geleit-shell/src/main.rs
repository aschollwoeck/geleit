//! GeleitMail — the Tauri shell (M9, ADR-0012).
//!
//! The window is the OS webview, and the UI inside it is `geleit-ui` (Leptos, Rust → WASM). This
//! binary owns exactly two things: the window, and the typed IPC seam to the engine ([`ipc`]).
//!
//! Why a webview at all, given constitution P4 once said the opposite: two attempts at native HTML
//! rendering failed on their merits (an embedded webview crashed on X11; a pure-Rust CPU renderer
//! could not render real mail). Rendering hostile mail HTML correctly *and* safely needs a real
//! browser engine. P4 was amended in M9 to say so honestly. Mail itself never renders in *this*
//! document — S9.2 confines every message to a script-free, CSP-locked iframe.
//!
//! Runs alongside the Slint `geleit-app` until S9.7's teardown, so the shipped app keeps working
//! throughout the migration.
mod dto;
mod ipc;

use geleit_platform::os_secret::OsSecretStore;
use ipc::AppState;
use std::sync::Arc;

fn main() {
    // Same dev bridge as the Slint app: `GELEIT_DB` overrides the mailbox path.
    let db_path = std::env::var("GELEIT_DB").unwrap_or_else(|_| "geleit.db".to_owned());
    let state = AppState {
        db_path,
        // The at-rest key + credentials live in the OS keychain (SEC-2/SEC-1, ADR-0008).
        secrets: Arc::new(OsSecretStore::new()),
    };

    tauri::Builder::default()
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            ipc::list_accounts,
            ipc::list_folders,
            ipc::list_messages,
            ipc::open_message,
            ipc::theme,
            ipc::dev_open_message,
        ])
        .run(tauri::generate_context!())
        .expect("GeleitMail could not start its window.");
}
