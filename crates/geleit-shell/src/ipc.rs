//! The IPC seam — the **only** way the Leptos frontend touches data (S9.1, ADR-0012).
//!
//! Two rules hold this boundary honest:
//!
//! 1. **DTOs, not store types.** The frontend never sees `geleit_store` types, so the schema can
//!    evolve without breaking the UI, and the UI cannot reach into the store even by accident.
//! 2. **Never block the webview.** SQLite calls are blocking, so every command hops to a blocking
//!    thread (constitution P1: the UI never waits). `Store` is not `Sync`, so we hold only the
//!    `db_path` + secrets in app state and open it per call — SQLCipher open is cheap next to the
//!    ~630 ms the webview already spent booting.
use geleit_engine::localstore::open_store;
use geleit_platform::secret::SecretStore;
use geleit_store::Store;
use std::sync::Arc;

use crate::dto::{
    display_sender, display_subject, folder_rank, AccountDto, FolderDto, MessageBodyDto, MessageDto,
};

/// What the shell needs to reach the encrypted store. Cheap to clone into a blocking task.
#[derive(Clone)]
pub struct AppState {
    pub db_path: String,
    pub secrets: Arc<dyn SecretStore>,
}

/// Run a blocking store operation off the webview's event loop (P1).
async fn with_store<T, F>(state: AppState, f: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(&Store) -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(move || {
        let store = open_store(&state.db_path, &*state.secrets)?;
        f(&store)
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
        Ok(MessageBodyDto {
            id: header.id,
            subject: display_subject(header.subject.as_deref()),
            from: display_sender(header.from_name.as_deref(), header.from_addr.as_deref()),
            date: header.date,
            plain: body.plain,
            html: body.html,
        })
    })
    .await
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
/// Returns `None` in a release build: the env var is not even read, so it cannot influence a
/// shipped app.
#[tauri::command]
pub async fn dev_open_message() -> Option<i64> {
    if !cfg!(debug_assertions) {
        return None;
    }
    std::env::var("GELEIT_OPEN").ok()?.parse().ok()
}
