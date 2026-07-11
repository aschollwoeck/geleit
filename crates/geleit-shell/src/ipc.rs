//! The IPC seam — the **only** way the Leptos frontend touches data (S9.1, ADR-0012).
//!
//! Two rules hold this boundary honest:
//!
//! 1. **DTOs, not store types.** The frontend never sees `geleit_store` types, so the schema can
//!    evolve without breaking the UI, and the UI cannot reach into the store even by accident.
//! 2. **Never block the webview.** SQLite calls are blocking, so every command hops to a blocking
//!    thread (constitution P1: the UI never waits).
//!
//! The store is opened **once** and kept. An earlier version of this file opened it per command,
//! which was quietly awful: each `open_store` does a Secret Service (DBus) round-trip for the
//! at-rest key, then `migrate()` and an FTS-backfill check. Boot alone fires five commands, and
//! every folder click paid it again — and against a *locked* keyring each one can block or prompt.
//! That is a per-interaction cost, and P3 says a latency regression is a defect.
//!
//! `Store` isn't `Sync` (it owns a rusqlite `Connection`), so it lives behind a `Mutex`. Queries here
//! are sub-millisecond local reads, so the lock is never meaningfully contended; if that ever changes,
//! reach for a connection pool — not for re-opening the database.
use geleit_engine::localstore::open_store;
use geleit_platform::secret::SecretStore;
use geleit_store::Store;
use std::sync::{Arc, Mutex};

use crate::dto::{
    display_sender, display_subject, folder_rank, AccountDto, FolderDto, MessageBodyDto, MessageDto,
};

/// What the shell needs to reach the encrypted store. Cheap to clone into a blocking task.
#[derive(Clone)]
pub struct AppState {
    pub db_path: String,
    pub secrets: Arc<dyn SecretStore>,
    /// Opened lazily on the first command, then reused. Lazy (rather than opened in `main`) so a
    /// locked keychain surfaces as a calm in-app message instead of the window failing to appear.
    store: Arc<Mutex<Option<Store>>>,
}

impl AppState {
    pub fn new(db_path: String, secrets: Arc<dyn SecretStore>) -> Self {
        Self {
            db_path,
            secrets,
            store: Arc::new(Mutex::new(None)),
        }
    }
}

/// Run a blocking store operation off the webview's event loop (P1), against the one open store.
async fn with_store<T, F>(state: AppState, f: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(&Store) -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(move || {
        let mut guard = state
            .store
            .lock()
            .map_err(|_| "The mailbox is unavailable.".to_owned())?;
        if guard.is_none() {
            *guard = Some(open_store(&state.db_path, &*state.secrets)?);
        }
        let store = guard.as_ref().expect("just opened");
        f(store)
    })
    .await
    .map_err(|_| "The mailbox task stopped unexpectedly.".to_owned())?
}

#[tauri::command]
pub async fn list_accounts(state: tauri::State<'_, AppState>) -> Result<Vec<AccountDto>, String> {
    with_store(state.inner().clone(), |store| {
        let accounts = store
            .list_accounts()
            .map_err(|_| "Couldn't read your accounts.".to_owned())?;
        Ok(accounts
            .into_iter()
            .map(|a| AccountDto {
                id: a.id,
                email: a.email,
                display_name: a.display_name,
            })
            .collect())
    })
    .await
}

#[tauri::command]
pub async fn list_folders(
    state: tauri::State<'_, AppState>,
    account_id: i64,
) -> Result<Vec<FolderDto>, String> {
    with_store(state.inner().clone(), move |store| {
        let mut folders = store
            .folders_for_account(account_id)
            .map_err(|_| "Couldn't read your folders.".to_owned())?;
        folders.sort_by(|a, b| {
            folder_rank(&a.name)
                .cmp(&folder_rank(&b.name))
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        Ok(folders
            .into_iter()
            .map(|f| FolderDto {
                id: f.id,
                name: f.name,
            })
            .collect())
    })
    .await
}

#[tauri::command]
pub async fn list_messages(
    state: tauri::State<'_, AppState>,
    folder_id: i64,
    limit: i64,
) -> Result<Vec<MessageDto>, String> {
    with_store(state.inner().clone(), move |store| {
        let headers = store
            .messages_in_folder(folder_id, limit.clamp(1, 5_000))
            .map_err(|_| "Couldn't read this folder.".to_owned())?;
        Ok(headers.into_iter().map(MessageDto::from).collect())
    })
    .await
}

#[tauri::command]
pub async fn open_message(
    state: tauri::State<'_, AppState>,
    id: i64,
) -> Result<MessageBodyDto, String> {
    with_store(state.inner().clone(), move |store| {
        let header = store
            .header_by_id(id)
            .map_err(|_| "Couldn't open this message.".to_owned())?
            .ok_or_else(|| "That message is no longer here.".to_owned())?;
        let body = store
            .body_for(id)
            .map_err(|_| "Couldn't read this message.".to_owned())?
            .unwrap_or_default();
        // Opening a message marks it read (READ-7) — persisted here, not just in the UI's signal,
        // or the unread dot reappears the moment the folder is re-listed from SQLite. (Server
        // write-back of \Seen is S9.4; the *local* write belongs here and nowhere else.) A failure
        // to record it must not stop the user reading their mail, so it is best-effort.
        let _ = store.set_seen(id, true);
        // The HTML body is NOT returned to the frontend. It is served straight to the sandboxed
        // iframe from the `mail://` origin (see `mailproto`), so hostile markup never enters the
        // app's own document — not even as a string in a signal.
        let is_html = body.html.is_some();
        let has_remote = body
            .html
            .as_deref()
            .is_some_and(geleit_engine::safehtml::has_remote_content);
        Ok(MessageBodyDto {
            id: header.id,
            subject: display_subject(header.subject.as_deref()),
            from: display_sender(header.from_name.as_deref(), header.from_addr.as_deref()),
            date: header.date,
            plain: body.plain,
            is_html,
            has_remote,
        })
    })
    .await
}

/// Fetch a message's sanitized HTML body for the `mail://` protocol handler. Blocking (SQLite), so
/// the handler runs it on a worker thread.
pub fn message_html(state: &AppState, id: i64, allow_remote: bool) -> Option<String> {
    let mut guard = state.store.lock().ok()?;
    if guard.is_none() {
        *guard = Some(open_store(&state.db_path, &*state.secrets).ok()?);
    }
    let html = guard.as_ref()?.body_for(id).ok()??.html?;
    let sanitized = if allow_remote {
        geleit_engine::safehtml::sanitize_html_allowing_remote(&html)
    } else {
        geleit_engine::safehtml::sanitize_html(&html)
    };
    Some(geleit_engine::safehtml::webview_document(
        &sanitized,
        allow_remote,
    ))
}

/// The persisted theme (`"dark"` / `"light"`), or `None` if the user has never chosen one.
///
/// The store is the source of truth — the same `setting` row the Slint app writes — so a user's
/// choice survives the M9 migration instead of silently reverting. `index.html` paints an *optimistic*
/// theme from `localStorage` before first paint (it cannot await IPC and still be instant); the app
/// reconciles against this on mount. The settings UI itself is S9.6.
#[tauri::command]
pub async fn theme(state: tauri::State<'_, AppState>) -> Result<Option<String>, String> {
    with_store(state.inner().clone(), |store| {
        store
            .get_setting("theme")
            .map_err(|_| "Couldn't read your settings.".to_owned())
    })
    .await
}

/// Dev/test seam: in a **debug** build, `GELEIT_OPEN=<message id>` makes the UI open that message on
/// boot. This exists because the app cannot be driven by injected input in the build environment
/// (no `xdotool`), so it is the only way to screenshot-verify the reading pane — and S9.2's whole
/// job is rendered mail, which *must* be verified visually.
///
/// **Compiled out of release builds entirely** — the command is not even registered, so the env var
/// cannot influence a shipped app. (It was previously gated with a runtime `cfg!(debug_assertions)`,
/// which is a *profile flag*, not a synonym for "debug build": turning on `debug-assertions` under
/// `[profile.release]` — routine when profiling — would have re-armed the seam in a real artifact.)
/// The frontend ignores the resulting "unknown command" error in release.
#[cfg(debug_assertions)]
#[tauri::command]
pub async fn dev_open_message() -> Option<i64> {
    std::env::var("GELEIT_OPEN").ok()?.parse().ok()
}

/// Dev/test seam, debug builds only: `GELEIT_IMAGES=1` opts the auto-opened message in to remote
/// images, so the PRIV-2 path can be screenshot-verified without a click. Never in release.
#[cfg(debug_assertions)]
#[tauri::command]
pub async fn dev_load_images() -> bool {
    std::env::var("GELEIT_IMAGES").is_ok_and(|v| v == "1")
}
