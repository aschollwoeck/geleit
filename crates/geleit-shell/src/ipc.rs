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
        let mut dtos: Vec<MessageDto> = headers.iter().cloned().map(MessageDto::from).collect();
        crate::dto::with_thread_counts(&headers, &mut dtos);
        Ok(dtos)
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

/// Kick a server write-back on a **detached background thread** (M5 model). The local store was
/// already updated optimistically; this reconciles the server and may outlive the command. A failure
/// is swallowed here — the next refresh restores truth — but never lets it lose mail (the callers
/// never expunge on the optimistic path).
fn spawn_writeback<F>(state: &AppState, f: F)
where
    F: FnOnce(&str, &dyn SecretStore) -> Result<(), String> + Send + 'static,
{
    let (db_path, secrets) = (state.db_path.clone(), state.secrets.clone());
    std::thread::spawn(move || {
        let _ = f(&db_path, &*secrets);
    });
}

/// Star / unstar a message (ORG-4). Optimistic local write + server write-back.
#[tauri::command]
pub async fn set_star(state: tauri::State<'_, AppState>, id: i64, on: bool) -> Result<(), String> {
    let st = state.inner().clone();
    let loc = with_store(st.clone(), move |store| {
        store
            .set_flagged(id, on)
            .map_err(|_| "Couldn't update the star.".to_owned())?;
        store
            .message_location(id)
            .map_err(|_| "Couldn't update the star.".to_owned())
    })
    .await?;
    if let Some((folder, uid)) = loc {
        spawn_writeback(&st, move |db, secrets| {
            geleit_engine::sync_actions::run_set_flag(
                db,
                secrets,
                account_of(db, secrets, id)?,
                &folder,
                uid as u32,
                on,
            )
        });
    }
    Ok(())
}

/// Mark a message unread again (READ-7). Optimistic local write + server write-back.
#[tauri::command]
pub async fn set_unread(state: tauri::State<'_, AppState>, id: i64) -> Result<(), String> {
    let st = state.inner().clone();
    let loc = with_store(st.clone(), move |store| {
        store
            .set_seen(id, false)
            .map_err(|_| "Couldn't mark unread.".to_owned())?;
        store
            .message_location(id)
            .map_err(|_| "Couldn't mark unread.".to_owned())
    })
    .await?;
    if let Some((folder, uid)) = loc {
        spawn_writeback(&st, move |db, secrets| {
            geleit_engine::sync_actions::run_set_seen(
                db,
                secrets,
                account_of(db, secrets, id)?,
                &folder,
                uid as u32,
                false,
            )
        });
    }
    Ok(())
}

/// Move a message to a well-known folder by role — archive / trash / spam / un-spam (ORG-1/2/3).
/// Removes it from the current folder locally (optimistic) and moves it on the server. Returns
/// whether it acted (false = the account has no such folder, so nothing was done).
#[tauri::command]
pub async fn move_to_role(
    state: tauri::State<'_, AppState>,
    id: i64,
    role: String,
) -> Result<bool, String> {
    use crate::dto::{resolve_folder, FolderRole};
    let role = match role.as_str() {
        "archive" => FolderRole::Archive,
        "trash" => FolderRole::Trash,
        "spam" => FolderRole::Spam,
        "inbox" => FolderRole::Inbox,
        _ => return Err("Unknown action.".to_owned()),
    };
    let st = state.inner().clone();

    // Plan the move — but do NOT delete the local row yet. The safety net for an optimistic local
    // delete ("self-heals on the next refresh") doesn't exist until S9.4, so deleting first would
    // leave a failed move absent locally with no way back. Instead the local delete happens *after*
    // the server confirms, below — so a failed move never removes the row from the store at all.
    let plan = with_store(st.clone(), move |store| {
        let Some((source, uid)) = store
            .message_location(id)
            .map_err(|_| "Couldn't move the message.".to_owned())?
        else {
            return Ok(None); // no server location (e.g. a local Saved message) — nothing to move
        };
        let account_id = store
            .account_for_message(id)
            .map_err(|_| "Couldn't move the message.".to_owned())?
            .ok_or_else(|| "Couldn't move the message.".to_owned())?;
        let folders: Vec<String> = store
            .folders_for_account(account_id)
            .map_err(|_| "Couldn't move the message.".to_owned())?
            .into_iter()
            .map(|f| f.name)
            .collect();
        let Some(target) = resolve_folder(&folders, role) else {
            return Ok(None); // account has no such folder — decline rather than invent one
        };
        if target == source {
            return Ok(None); // already there
        }
        Ok(Some((account_id, source, uid, target.to_owned())))
    })
    .await?;

    let Some((account_id, source, uid, target)) = plan else {
        return Ok(false);
    };

    // Do the server move first, on a blocking thread. `uid_mv` is a single server-atomic
    // copy-and-remove, so there is no window where the message is duplicated or lost.
    let (db, secrets) = (st.db_path.clone(), st.secrets.clone());
    let (src, tgt) = (source.clone(), target.clone());
    let moved = tauri::async_runtime::spawn_blocking(move || {
        geleit_engine::sync_actions::run_move(&db, &*secrets, account_id, &src, uid as u32, &tgt)
    })
    .await
    .map_err(|_| "The mailbox task stopped unexpectedly.".to_owned())?;
    moved?; // a server failure returns here with the row still present locally — nothing lost

    // Only now remove it locally, so the store and server agree. It reappears in the target folder
    // on the next sync; it is never expunged, so no mail is lost.
    with_store(st, move |store| {
        store
            .delete_message(id)
            .map_err(|_| "Couldn't update the local mailbox.".to_owned())
    })
    .await?;
    Ok(true)
}

/// The account a message belongs to — read once inside a write-back (its own store connection).
fn account_of(db: &str, secrets: &dyn SecretStore, id: i64) -> Result<i64, String> {
    let store = open_store(db, secrets)?;
    store
        .account_for_message(id)
        .ok()
        .flatten()
        .ok_or_else(|| "unknown account".to_owned())
}

/// Refresh an account's folder: sync the folder list + the current folder's recent envelopes, then
/// backfill older mail in the background, emitting `sync-progress` events as batches land (P1 — the
/// UI never blocks; feedback streams instead). Returns when the *recent* sync is done; the backfill
/// keeps running and emitting. A network failure is reported calmly and leaves local mail untouched.
#[tauri::command]
pub async fn refresh(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    account_id: i64,
    folder: String,
) -> Result<(), String> {
    use tauri::Emitter;
    let (db, secrets) = (state.db_path.clone(), state.secrets.clone());

    // Phase 1 — recent mail. Await this so the caller can re-list once it's in.
    let (db1, secrets1, folder1) = (db.clone(), secrets.clone(), folder.clone());
    let recent = tauri::async_runtime::spawn_blocking(move || {
        geleit_engine::sync_actions::run_refresh(&db1, &*secrets1, account_id, &folder1)
    })
    .await
    .map_err(|_| "The sync task stopped unexpectedly.".to_owned())?;
    recent?;

    // Phase 2 — backfill older mail in the background, streaming progress. Detached: it may outlive
    // the command, and the UI shouldn't wait on it.
    std::thread::spawn(move || {
        // A drop guard emits the completion sentinel **no matter how the thread leaves** — including a
        // panic — so the UI's progress strip can never get stuck. `-1` = finished cleanly, `-2` = it
        // stopped early (so the UI can show a calm "will resume next refresh" note, S9.4-4).
        struct Done {
            app: tauri::AppHandle,
            code: i64,
        }
        impl Drop for Done {
            fn drop(&mut self) {
                let _ = self.app.emit("sync-progress", self.code);
            }
        }
        let mut done = Done {
            app: app.clone(),
            code: -2,
        };

        let mut emit = |count: usize| {
            let _ = app.emit("sync-progress", count as i64);
        };
        if geleit_engine::sync_actions::run_backfill(
            &db, &*secrets, account_id, &folder, 200, &mut emit,
        )
        .is_ok()
        {
            done.code = -1; // clean finish
        }
        // `done` drops here (or on a panic unwinding through this scope), emitting its code.
    });
    Ok(())
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
