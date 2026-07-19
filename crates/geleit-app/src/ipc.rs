//! The Tauri adapter over the host-agnostic core (ADR-0014).
//!
//! Every command's *logic* lives in [`geleit_host::commands`]; this module is the thin Tauri side of
//! the IPC seam. It does three things and nothing else:
//!
//! 1. Wraps each `geleit-host` command in a `#[tauri::command]` that Tauri's `generate_handler!` can
//!    register — mapping `tauri::State`/`AppHandle` onto the plain `&AppState`/[`geleit_host::Shell`]
//!    the core expects. The macros below keep those wrappers to one line each.
//! 2. Implements [`geleit_host::Shell`] for the desktop shell ([`TauriShell`]): emit over the webview
//!    bridge, and reflect the unread badge in the window title + tray tooltip.
//! 3. Re-exports the host's worker helpers so `scheduler`/`idle`/`backfill`/`update`/`mailproto` keep
//!    reaching them as `ipc::…` unchanged.
//!
//! The two Tauri-only commands — the auto-updater's `check_update`/`install_update` — stay here in
//! full, because they *are* Tauri (`tauri-plugin-updater`); the web host stubs them.
use geleit_host::dto::{
    AccountDto, ComposeDraft, DraftSummary, ExportSummary, FolderDto, MessageBodyDto, MessageDto,
    OutboxItemDto, ResumedDraft, RuleDto, SnoozePresetDto, SnoozedItemDto,
};
pub use geleit_host::AppState;

// The two host helpers the desktop shell still reaches directly: `bool_setting` (update.rs, the
// auto-update opt-out) and `message_html` (mailproto, the mail:// origin). The background workers used
// to need the rest, but they now live in geleit-host and call the core directly (ADR-0014).
pub use geleit_host::commands::{bool_setting, message_html};

/// The desktop host's [`Shell`](geleit_host::Shell): a live `AppHandle` to emit events on and to hang
/// the unread badge from.
#[derive(Clone)]
pub struct TauriShell {
    app: tauri::AppHandle,
}

impl TauriShell {
    #[must_use]
    pub fn new(app: tauri::AppHandle) -> Self {
        Self { app }
    }
}

impl geleit_host::Shell for TauriShell {
    fn emit(&self, event: &str, payload: serde_json::Value) {
        use tauri::Emitter;
        let _ = self.app.emit(event, payload);
    }

    fn set_badge(&self, title: &str) {
        use tauri::Manager;
        if let Some(win) = self.app.get_webview_window("main") {
            let _ = win.set_title(title);
        }
        // Keep the tray tooltip in step with the title — same count, one source of truth. Hovering the
        // tray icon then says "GeleitMail — 3 unread" even while the window is hidden.
        if let Some(tray) = self.app.tray_by_id(crate::tray::TRAY_ID) {
            let _ = tray.set_tooltip(Some(title));
        }
    }
}

/// A command taking `&AppState` + plain args → a `#[tauri::command]` that unwraps the managed state.
macro_rules! cmd {
    ($name:ident ( $($arg:ident : $ty:ty),* $(,)? ) -> $ret:ty) => {
        // `send_message` legitimately takes the whole compose form; the others are well under the cap.
        #[allow(clippy::too_many_arguments)]
        #[tauri::command]
        pub async fn $name(state: tauri::State<'_, AppState>, $($arg: $ty),*) -> $ret {
            geleit_host::commands::$name(state.inner(), $($arg),*).await
        }
    };
}

/// A command taking no state at all (dev seams, native dialogs, snooze presets).
macro_rules! cmd_nostate {
    ($name:ident () -> $ret:ty) => {
        #[tauri::command]
        pub async fn $name() -> $ret {
            geleit_host::commands::$name().await
        }
    };
}

/// A command that also needs a [`Shell`](geleit_host::Shell) (to emit or to set the badge).
macro_rules! cmd_shell {
    ($name:ident ( $($arg:ident : $ty:ty),* $(,)? ) -> $ret:ty) => {
        #[tauri::command]
        pub async fn $name(
            app: tauri::AppHandle,
            state: tauri::State<'_, AppState>,
            $($arg: $ty),*
        ) -> $ret {
            geleit_host::commands::$name(&TauriShell::new(app), state.inner(), $($arg),*).await
        }
    };
}

// --- Reads + writes that need only the store -----------------------------------------------------
cmd!(list_accounts() -> Result<Vec<AccountDto>, String>);
cmd!(list_folders(account_id: i64) -> Result<Vec<FolderDto>, String>);
cmd!(list_messages(folder_id: i64, limit: i64) -> Result<Vec<MessageDto>, String>);
cmd!(list_all_messages(limit: i64) -> Result<Vec<MessageDto>, String>);
cmd!(open_message(id: i64, mark_read: bool) -> Result<MessageBodyDto, String>);
cmd!(set_star(id: i64, on: bool) -> Result<(), String>);
cmd!(set_read(id: i64) -> Result<(), String>);
cmd!(set_unread(id: i64) -> Result<(), String>);
cmd!(move_to_role(id: i64, role: String) -> Result<bool, String>);
cmd!(move_to_folder(id: i64, folder: String) -> Result<bool, String>);
cmd!(empty_trash(account_id: i64) -> Result<(), String>);
cmd!(delete_forever(id: i64) -> Result<(), String>);
cmd!(create_folder(account_id: i64, name: String) -> Result<i64, String>);
cmd!(rename_folder(account_id: i64, from: String, to: String) -> Result<(), String>);
cmd!(delete_folder(account_id: i64, folder_id: i64, name: String) -> Result<(), String>);
cmd!(compose_draft(id: i64, kind: String) -> Result<ComposeDraft, String>);
cmd!(send_message(
    account_id: i64, to: String, cc: String, subject: String, body: String,
    in_reply_to: Option<String>, references: Vec<String>, attachments: Vec<String>,
    markdown: bool, draft_id: Option<i64>, outbox_edit_id: Option<i64>
) -> Result<bool, String>);
cmd!(suggest_addresses(account_id: i64, prefix: String) -> Result<Vec<String>, String>);
cmd!(save_draft(
    account_id: i64, draft_id: Option<i64>, draft: ComposeDraft, attachments: Vec<String>
) -> Result<i64, String>);
cmd!(list_drafts(account_id: i64) -> Result<Vec<DraftSummary>, String>);
cmd!(load_draft(id: i64) -> Result<Option<ResumedDraft>, String>);
cmd!(refresh_drafts(account_id: i64) -> Result<bool, String>);
cmd!(resume_server_draft(id: i64) -> Result<ResumedDraft, String>);
cmd!(delete_draft(id: i64) -> Result<(), String>);
cmd!(purge_server_drafts(account_id: i64) -> Result<(), String>);
cmd!(save_eml(id: i64) -> Result<bool, String>);
cmd!(export_folder(folder_id: i64, folder_name: String) -> Result<Option<ExportSummary>, String>);
cmd!(export_account(account_id: i64) -> Result<Option<ExportSummary>, String>);
cmd!(open_eml_file(account_id: i64) -> Result<Option<i64>, String>);
cmd!(save_attachment(message_id: i64, index: usize) -> Result<bool, String>);
cmd!(search(account_id: i64, query: String) -> Result<Vec<MessageDto>, String>);
cmd!(search_all(query: String) -> Result<Vec<MessageDto>, String>);
cmd!(set_theme(theme: String) -> Result<(), String>);
cmd!(theme() -> Result<Option<String>, String>);
cmd!(remove_account(account_id: i64) -> Result<bool, String>);
cmd!(get_bool_setting(key: String) -> Result<Option<bool>, String>);
cmd!(set_bool_setting(key: String, value: bool) -> Result<(), String>);
cmd!(get_setting(key: String) -> Result<Option<String>, String>);
cmd!(set_setting(key: String, value: String) -> Result<(), String>);
cmd!(get_signature(account_id: i64) -> Result<String, String>);
cmd!(set_signature(account_id: i64, signature: String) -> Result<(), String>);
cmd!(outbox_status() -> Result<(i64, i64), String>);
cmd!(list_outbox() -> Result<Vec<OutboxItemDto>, String>);
cmd!(retry_outbox(id: i64) -> Result<(), String>);
cmd!(discard_outbox(id: i64) -> Result<(), String>);
cmd!(edit_outbox(id: i64) -> Result<Option<ResumedDraft>, String>);
cmd!(list_snoozed(account_id: i64) -> Result<Vec<SnoozedItemDto>, String>);
cmd!(list_rules(account_id: i64) -> Result<Vec<RuleDto>, String>);
cmd!(add_rule(
    account_id: i64, field: String, pattern: String, target_folder: Option<String>,
    mark_read: bool, star: bool
) -> Result<i64, String>);
cmd!(delete_rule(id: i64) -> Result<(), String>);
cmd!(move_rule(id: i64, up: bool) -> Result<(), String>);

// --- No-state: dialogs, snooze presets, dev seams ------------------------------------------------
cmd_nostate!(snooze_presets() -> Result<Vec<SnoozePresetDto>, String>);
cmd_nostate!(pick_files() -> Result<Vec<String>, String>);
#[cfg(debug_assertions)]
cmd_nostate!(dev_open_message() -> Option<i64>);
#[cfg(debug_assertions)]
cmd_nostate!(dev_load_images() -> bool);
#[cfg(debug_assertions)]
cmd_nostate!(dev_compose() -> Option<String>);
#[cfg(debug_assertions)]
cmd_nostate!(dev_unified() -> bool);
#[cfg(debug_assertions)]
cmd_nostate!(dev_setup() -> bool);
#[cfg(debug_assertions)]
cmd_nostate!(dev_settings() -> Option<String>);
#[cfg(debug_assertions)]
cmd_nostate!(dev_search() -> Option<String>);
#[cfg(debug_assertions)]
cmd_nostate!(dev_trash() -> Option<String>);
#[cfg(debug_assertions)]
cmd_nostate!(dev_compose_to() -> Option<String>);
#[cfg(debug_assertions)]
cmd_nostate!(dev_drafts() -> bool);
#[cfg(debug_assertions)]
cmd_nostate!(dev_resume() -> bool);
#[cfg(debug_assertions)]
cmd_nostate!(dev_select() -> Option<String>);
#[cfg(debug_assertions)]
cmd_nostate!(dev_folder() -> Option<String>);

// --- Commands that emit or set the badge ---------------------------------------------------------
cmd_shell!(update_badge() -> Result<(), String>);
cmd_shell!(snooze_messages(ids: Vec<i64>, until: i64) -> Result<(), String>);
cmd_shell!(unsnooze_message(id: i64) -> Result<(), String>);
cmd_shell!(run_rules_now(account_id: i64) -> Result<i64, String>);

// --- Hand-written: host-specific side-effects or Tauri-only plumbing -----------------------------

/// Refresh a folder. The detached backfill thread emits `sync-progress`, so the shell is passed as an
/// owned `Arc<dyn Shell>` it can move into that thread (see [`geleit_host::commands::refresh`]).
#[tauri::command]
pub async fn refresh(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    account_id: i64,
    folder: String,
) -> Result<(), String> {
    let shell = std::sync::Arc::new(TauriShell::new(app));
    geleit_host::commands::refresh(shell, state.inner(), account_id, folder).await
}

/// Add (or reconfigure) an account, then give it instant new-mail push right away — the one
/// host-specific side-effect the core leaves to us (idempotent; the background poll covers it too).
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn add_account(
    state: tauri::State<'_, AppState>,
    email: String,
    display_name: String,
    imap_host: String,
    imap_port: String,
    username: String,
    password: String,
    smtp_host: String,
    smtp_port: String,
    smtp_starttls: bool,
    signature: String,
    allow_invalid_certs: bool,
) -> Result<i64, String> {
    let account_id = geleit_host::commands::add_account(
        state.inner(),
        email,
        display_name,
        imap_host,
        imap_port,
        username,
        password,
        smtp_host,
        smtp_port,
        smtp_starttls,
        signature,
        allow_invalid_certs,
    )
    .await?;
    // Give the new account instant IMAP IDLE push right away — spawn its watcher on Tauri's runtime.
    // Idempotent (`None` if already watched); the background poll covers it regardless.
    if let Some(watcher) = geleit_host::worker::idle::watch_new_account(state.inner(), account_id) {
        tauri::async_runtime::spawn(watcher);
    }
    Ok(account_id)
}

/// The running app version (APP-7).
#[tauri::command]
pub fn app_version() -> String {
    geleit_host::commands::app_version()
}

/// Check the release feed (APP-7). Inherently Tauri (`tauri-plugin-updater`), so it stays here.
#[tauri::command]
pub async fn check_update(
    app: tauri::AppHandle,
) -> Result<Option<crate::update::UpdateInfo>, String> {
    crate::update::check(&app).await
}

/// Download, verify, and install the pending update, then relaunch (APP-7).
#[tauri::command]
pub async fn install_update(app: tauri::AppHandle) -> Result<(), String> {
    crate::update::install(&app).await
}
