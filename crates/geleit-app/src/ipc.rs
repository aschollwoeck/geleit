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
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::dto::{
    compose_from_draft, display_sender, display_subject, draft_content_from, folder_rank,
    human_size, is_protected_folder, safe_attachment_filename, safe_filename_stem,
    validate_folder_name, AccountDto, AttachmentDto, ComposeDraft, DraftSummary, FolderDto,
    MessageBodyDto, MessageDto, ResumedDraft,
};

/// One async lock per `(account, folder)` — the guard that keeps two syncs of one folder apart.
type SyncLocks = Arc<Mutex<HashMap<(i64, String), Arc<tokio::sync::Mutex<()>>>>>;

/// What the shell needs to reach the engine. Cheap to clone into a blocking task.
#[derive(Clone)]
pub struct AppState {
    pub db_path: String,
    pub secrets: Arc<dyn SecretStore>,
    /// Opened lazily on the first command, then reused. Lazy (rather than opened in `main`) so a
    /// locked keychain surfaces as a calm in-app message instead of the window failing to appear.
    store: Arc<Mutex<Option<Store>>>,
    /// One lock per `(account, folder)`, so **only one sync of a folder runs at a time** — see
    /// [`AppState::sync_lock`].
    sync_locks: SyncLocks,
    /// Poked when a user-pressed Refresh succeeds, to wake the background scheduler — see
    /// [`AppState::wake_sync`].
    wake_sync: Arc<tokio::sync::Notify>,
}

impl AppState {
    pub fn new(db_path: String, secrets: Arc<dyn SecretStore>) -> Self {
        Self {
            db_path,
            secrets,
            store: Arc::new(Mutex::new(None)),
            sync_locks: Arc::new(Mutex::new(HashMap::new())),
            wake_sync: Arc::new(tokio::sync::Notify::new()),
        }
    }

    /// The scheduler's wake-up call.
    ///
    /// A successful **user-pressed Refresh** is the strongest evidence we have that the network is
    /// back — the user just proved it. So it wakes the scheduler, which resets its backoff and sweeps
    /// immediately. Without this, a laptop that was offline overnight (backed off to the half-hour
    /// cap) would leave background mail stale for up to 30 minutes after the lid opens — and
    /// `tokio::time::sleep` is monotonic, so a suspended machine doesn't even burn that time down
    /// while it's asleep.
    pub(crate) fn wake_sync(&self) -> Arc<tokio::sync::Notify> {
        Arc::clone(&self.wake_sync)
    }

    /// The lock for one folder's sync. **Every** sync path takes it — the background scheduler and a
    /// user-pressed Refresh alike — so the two can never run over each other.
    ///
    /// This matters more than it looks. Without it, both would compute "what's new" from the same
    /// local snapshot, both would fetch the same messages, and (once slice 3 lands) the user would get
    /// **two notifications for one email**. Serializing also means the second sync sees the mail the
    /// first one stored, so it simply finds nothing new — which is why waiting, rather than skipping,
    /// is the right behaviour: a Refresh pressed during a background sync still ends with fresh mail
    /// on screen, it just queues behind it.
    ///
    /// The map only grows with folders actually synced (a handful), so it is never pruned.
    fn sync_lock(&self, account_id: i64, folder: &str) -> Arc<tokio::sync::Mutex<()>> {
        let mut locks = self
            .sync_locks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Arc::clone(locks.entry((account_id, folder.to_owned())).or_default())
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
        // The provider's Drafts folder is not a rail entry of its own: the **Drafts** entry *is* it
        // (its contents are merged into that list by `list_drafts`). Leaving it here would show
        // "Drafts" twice — once as the folder, once as the list of what's in it.
        let names: Vec<String> = folders.iter().map(|f| f.name.clone()).collect();
        if let Some(drafts) = crate::dto::pick_drafts_folder(&names) {
            folders.retain(|f| f.name != drafts);
        }
        folders.sort_by(|a, b| {
            folder_rank(&a.name)
                .cmp(&folder_rank(&b.name))
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        // Per-folder unread counts, folded onto the folders (0 when a folder has none).
        let counts: std::collections::HashMap<i64, i64> = store
            .folder_unread_counts(account_id)
            .unwrap_or_default()
            .into_iter()
            .collect();
        Ok(folders
            .into_iter()
            .map(|f| FolderDto {
                unread: counts.get(&f.id).copied().unwrap_or(0),
                id: f.id,
                name: f.name,
            })
            .collect())
    })
    .await
}

/// Remove an account from this device (SEC-3): keychain password + local mail. Worker (keychain +
/// SQLite). Returns whether the keychain password was cleared cleanly.
#[tauri::command]
pub async fn remove_account(
    state: tauri::State<'_, AppState>,
    account_id: i64,
) -> Result<bool, String> {
    let (db, secrets) = (state.db_path.clone(), state.secrets.clone());
    tauri::async_runtime::spawn_blocking(move || {
        geleit_engine::sync_actions::run_remove_account(&db, &*secrets, account_id)
    })
    .await
    .map_err(|_| "The task stopped unexpectedly.".to_owned())?
}

/// A boolean setting persisted in the store's `setting` k/v table (block-remote-images, mark-read,
/// notify). Read/written by the settings window; defaults handled on the frontend.
#[tauri::command]
pub async fn get_bool_setting(
    state: tauri::State<'_, AppState>,
    key: String,
) -> Result<Option<bool>, String> {
    with_store(state.inner().clone(), move |store| {
        Ok(store
            .get_setting(&key)
            .map_err(|_| "Couldn't read your settings.".to_owned())?
            .map(|v| v == "1" || v == "true"))
    })
    .await
}

#[tauri::command]
pub async fn set_bool_setting(
    state: tauri::State<'_, AppState>,
    key: String,
    value: bool,
) -> Result<(), String> {
    with_store(state.inner().clone(), move |store| {
        store
            .set_setting(&key, if value { "1" } else { "0" })
            .map_err(|_| "Couldn't save your setting.".to_owned())
    })
    .await
}

/// The account's signature (for the settings editor). `set_signature` persists it.
#[tauri::command]
pub async fn get_signature(
    state: tauri::State<'_, AppState>,
    account_id: i64,
) -> Result<String, String> {
    with_store(state.inner().clone(), move |store| {
        Ok(store
            .signature(account_id)
            .map_err(|_| "Couldn't read your signature.".to_owned())?
            .unwrap_or_default())
    })
    .await
}

#[tauri::command]
pub async fn set_signature(
    state: tauri::State<'_, AppState>,
    account_id: i64,
    signature: String,
) -> Result<(), String> {
    with_store(state.inner().clone(), move |store| {
        store
            .update_signature(account_id, &signature)
            .map_err(|_| "Couldn't save your signature.".to_owned())
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

/// The merged "All inboxes" listing — every account's INBOX, newest first, each row tagged with its
/// account so the UI can show which mailbox it came from. No thread counts (a thread is per-account).
#[tauri::command]
pub async fn list_all_messages(
    state: tauri::State<'_, AppState>,
    limit: i64,
) -> Result<Vec<MessageDto>, String> {
    with_store(state.inner().clone(), move |store| {
        let rows = store
            .messages_in_all_inboxes(limit.clamp(1, 5_000))
            .map_err(|_| "Couldn't read your inboxes.".to_owned())?;
        Ok(rows
            .into_iter()
            .map(|(h, account)| {
                let mut dto = MessageDto::from(h);
                dto.account = account;
                dto
            })
            .collect())
    })
    .await
}

#[tauri::command]
pub async fn open_message(
    state: tauri::State<'_, AppState>,
    id: i64,
    mark_read: bool,
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
        // to record it must not stop the user reading their mail, so it is best-effort. When the
        // "mark as read when opened" preference is off, the read is skipped entirely.
        if mark_read {
            let _ = store.set_seen(id, true);
        }
        // The HTML body is NOT returned to the frontend. It is served straight to the sandboxed
        // iframe from the `mail://` origin (see `mailproto`), so hostile markup never enters the
        // app's own document — not even as a string in a signal.
        let is_html = body.html.is_some();
        let has_remote = body
            .html
            .as_deref()
            .is_some_and(geleit_engine::safehtml::has_remote_content);
        // Attachment metadata for the reading pane (bytes fetched on demand to save). Order matches
        // the parse order, so each row's index is its save key.
        let attachments = store
            .attachments_for(id)
            .unwrap_or_default()
            .into_iter()
            .enumerate()
            .map(|(i, a)| AttachmentDto {
                name: a
                    .filename
                    .unwrap_or_else(|| format!("attachment {}", i + 1)),
                size: human_size(a.size),
            })
            .collect();
        Ok(MessageBodyDto {
            id: header.id,
            subject: display_subject(header.subject.as_deref()),
            from: display_sender(header.from_name.as_deref(), header.from_addr.as_deref()),
            date: header.date,
            plain: body.plain,
            is_html,
            has_remote,
            attachments,
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

/// Mark a message read (READ-7, for bulk mark-read). Optimistic local write + server write-back.
#[tauri::command]
pub async fn set_read(state: tauri::State<'_, AppState>, id: i64) -> Result<(), String> {
    set_seen_and_writeback(state, id, true, "Couldn't mark read.").await
}

/// Mark a message unread again (READ-7). Optimistic local write + server write-back.
#[tauri::command]
pub async fn set_unread(state: tauri::State<'_, AppState>, id: i64) -> Result<(), String> {
    set_seen_and_writeback(state, id, false, "Couldn't mark unread.").await
}

/// Shared body for `set_read`/`set_unread`: persist the seen flag locally, then write it back to the
/// server (`\Seen`) on a worker, targeting the message's real folder.
async fn set_seen_and_writeback(
    state: tauri::State<'_, AppState>,
    id: i64,
    seen: bool,
    err: &'static str,
) -> Result<(), String> {
    let st = state.inner().clone();
    let loc = with_store(st.clone(), move |store| {
        store.set_seen(id, seen).map_err(|_| err.to_owned())?;
        store.message_location(id).map_err(|_| err.to_owned())
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
                seen,
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

/// Empty the account's Trash (ORG-2): permanently delete everything in it, on the server and locally.
/// Irreversible — the UI confirms first.
#[tauri::command]
pub async fn empty_trash(state: tauri::State<'_, AppState>, account_id: i64) -> Result<(), String> {
    use crate::dto::{resolve_folder, FolderRole};
    let st = state.inner().clone();
    // Resolve the Trash folder (name for the server call, id for the local clear).
    let trash = with_store(st.clone(), move |store| {
        let folders = store
            .folders_for_account(account_id)
            .map_err(|_| "Couldn't read your folders.".to_owned())?;
        let names: Vec<String> = folders.iter().map(|f| f.name.clone()).collect();
        Ok(resolve_folder(&names, FolderRole::Trash).and_then(|name| {
            folders
                .iter()
                .find(|f| f.name == name)
                .map(|f| (name.to_owned(), f.id))
        }))
    })
    .await?;
    let Some((name, folder_id)) = trash else {
        return Err("This account has no Trash folder.".to_owned());
    };
    // Empty on the server first (blocking); only then clear the local rows, so a failure keeps them.
    let (db, secrets) = (st.db_path.clone(), st.secrets.clone());
    tauri::async_runtime::spawn_blocking(move || {
        geleit_engine::sync_actions::run_empty_folder(&db, &*secrets, account_id, &name)
    })
    .await
    .map_err(|_| "The mailbox task stopped unexpectedly.".to_owned())??;
    with_store(st, move |store| {
        store
            .delete_folder_messages(folder_id)
            .map(|_| ())
            .map_err(|_| "Couldn't clear the local Trash.".to_owned())
    })
    .await
}

/// Permanently delete a single message that is already in Trash (ORG-2). Irreversible — the UI
/// confirms first. A message with no server location (local-only, or already expunged) skips the
/// server step but is still removed locally.
#[tauri::command]
pub async fn delete_forever(state: tauri::State<'_, AppState>, id: i64) -> Result<(), String> {
    let st = state.inner().clone();
    let plan = with_store(st.clone(), move |store| {
        let loc = store
            .message_location(id)
            .map_err(|_| "Couldn't delete the message.".to_owned())?;
        let acc = store
            .account_for_message(id)
            .map_err(|_| "Couldn't delete the message.".to_owned())?;
        Ok(loc.zip(acc))
    })
    .await?;
    // A message with no server location (local-only, or already expunged) still needs its local row
    // removed — otherwise "Delete forever" reports success but the message reappears on the next sync.
    if let Some(((folder, uid), account_id)) = plan {
        let (db, secrets) = (st.db_path.clone(), st.secrets.clone());
        tauri::async_runtime::spawn_blocking(move || {
            geleit_engine::sync_actions::run_delete_permanently(
                &db, &*secrets, account_id, &folder, uid as u32,
            )
        })
        .await
        .map_err(|_| "The mailbox task stopped unexpectedly.".to_owned())??;
    }
    with_store(st, move |store| {
        store
            .delete_message(id)
            .map_err(|_| "Couldn't update the local mailbox.".to_owned())
    })
    .await
}

/// Create a folder (ORG-6): create it on the server, then add the local row. Returns the new folder
/// id. Rejects a blank/slashed name.
#[tauri::command]
pub async fn create_folder(
    state: tauri::State<'_, AppState>,
    account_id: i64,
    name: String,
) -> Result<i64, String> {
    let name = validate_folder_name(&name)?;
    let st = state.inner().clone();
    let (db, secrets) = (st.db_path.clone(), st.secrets.clone());
    let name2 = name.clone();
    tauri::async_runtime::spawn_blocking(move || {
        geleit_engine::sync_actions::run_create_folder(&db, &*secrets, account_id, &name2)
    })
    .await
    .map_err(|_| "The folder task stopped unexpectedly.".to_owned())??;
    with_store(st, move |store| {
        store
            .upsert_folder(account_id, &name)
            .map_err(|_| "Created on the server, but couldn't add it locally.".to_owned())
    })
    .await
}

/// Rename a folder (ORG-6): rename it on the server, then rename the local row in place (its messages
/// stay attached). Protected folders (Inbox, roles, Saved/Drafts) are refused.
#[tauri::command]
pub async fn rename_folder(
    state: tauri::State<'_, AppState>,
    account_id: i64,
    from: String,
    to: String,
) -> Result<(), String> {
    if is_protected_folder(&from) {
        return Err("That folder can't be renamed.".to_owned());
    }
    let to = validate_folder_name(&to)?;
    // Don't let a user rename an ordinary folder *into* a reserved name — that would mint a
    // role-named folder the UI then treats as protected (un-renamable, un-deletable).
    if is_protected_folder(&to) {
        return Err("That name is reserved for a standard folder.".to_owned());
    }
    let st = state.inner().clone();
    let (db, secrets) = (st.db_path.clone(), st.secrets.clone());
    let (from2, to2) = (from.clone(), to.clone());
    tauri::async_runtime::spawn_blocking(move || {
        geleit_engine::sync_actions::run_rename_folder(&db, &*secrets, account_id, &from2, &to2)
    })
    .await
    .map_err(|_| "The folder task stopped unexpectedly.".to_owned())??;
    with_store(st, move |store| {
        store
            .rename_folder(account_id, &from, &to)
            .map(|_| ())
            .map_err(|_| "Renamed on the server, but couldn't update it locally.".to_owned())
    })
    .await
}

/// Delete a folder (ORG-6): delete it on the server, then remove the local row (cascading its
/// messages). Protected folders are refused. Irreversible — the UI confirms first.
#[tauri::command]
pub async fn delete_folder(
    state: tauri::State<'_, AppState>,
    account_id: i64,
    folder_id: i64,
    name: String,
) -> Result<(), String> {
    if is_protected_folder(&name) {
        return Err("That folder can't be deleted.".to_owned());
    }
    let st = state.inner().clone();
    let (db, secrets) = (st.db_path.clone(), st.secrets.clone());
    let name2 = name.clone();
    tauri::async_runtime::spawn_blocking(move || {
        geleit_engine::sync_actions::run_delete_folder(&db, &*secrets, account_id, &name2)
    })
    .await
    .map_err(|_| "The folder task stopped unexpectedly.".to_owned())??;
    with_store(st, move |store| {
        store
            .delete_folder(folder_id)
            .map_err(|_| "Deleted on the server, but couldn't remove it locally.".to_owned())
    })
    .await
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

/// Add (or reconnect) an account: validate the form, create the account, store the password in the
/// keychain, and do a first sync (S9.6). Worker — network + keychain (P1). Returns the account id.
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
    use geleit_engine::sync_actions::{build_settings, build_smtp_settings};
    // Validate the form up front (pure) — a bad field is a calm message, not a failed connection.
    let (email, imap) = build_settings(
        &email,
        &imap_host,
        &imap_port,
        &username,
        allow_invalid_certs,
    )?;
    let smtp = build_smtp_settings(&smtp_host, &smtp_port, smtp_starttls)?;
    let display = (!display_name.trim().is_empty()).then(|| display_name.trim().to_owned());

    let (db, secrets) = (state.db_path.clone(), state.secrets.clone());
    tauri::async_runtime::spawn_blocking(move || {
        geleit_engine::sync_actions::run_setup(
            &db,
            &*secrets,
            &email,
            display.as_deref(),
            imap,
            smtp,
            &signature,
            &password,
        )
    })
    .await
    .map_err(|_| "The setup task stopped unexpectedly.".to_owned())?
}

/// Search an account's mail (FTS5, M6). Instant + local (P1); supports `from:`/`subject:`/
/// `has:attachment` operators. Returns headers as list rows.
#[tauri::command]
pub async fn search(
    state: tauri::State<'_, AppState>,
    account_id: i64,
    query: String,
) -> Result<Vec<MessageDto>, String> {
    with_store(state.inner().clone(), move |store| {
        let headers = store
            .search_messages(account_id, &query, 300)
            .map_err(|_| "Couldn't search your mail.".to_owned())?;
        let mut dtos: Vec<MessageDto> = headers.iter().cloned().map(MessageDto::from).collect();
        crate::dto::with_thread_counts(&headers, &mut dtos);
        Ok(dtos)
    })
    .await
}

/// Search every account's mail at once — for the merged "All inboxes" view. Rows are tagged with
/// their account (no thread counts: a thread is per-account).
#[tauri::command]
pub async fn search_all(
    state: tauri::State<'_, AppState>,
    query: String,
) -> Result<Vec<MessageDto>, String> {
    with_store(state.inner().clone(), move |store| {
        let rows = store
            .search_all_accounts(&query, 300)
            .map_err(|_| "Couldn't search your mail.".to_owned())?;
        Ok(rows
            .into_iter()
            .map(|(h, account)| {
                let mut dto = MessageDto::from(h);
                dto.account = account;
                dto
            })
            .collect())
    })
    .await
}

/// Persist the theme choice (`"dark"` / `"light"`) — the same `setting` row S9.1 reads on boot, so a
/// choice survives restart. The frontend already flipped the document; this makes it stick.
#[tauri::command]
pub async fn set_theme(state: tauri::State<'_, AppState>, theme: String) -> Result<(), String> {
    with_store(state.inner().clone(), move |store| {
        store
            .set_setting("theme", &theme)
            .map_err(|_| "Couldn't save your setting.".to_owned())
    })
    .await
}

/// Build a reply / reply-all / forward draft, prefilled from a stored message (S9.5). Pure over the
/// store — no network. `kind` is "reply" | "reply_all" | "forward".
#[tauri::command]
pub async fn compose_draft(
    state: tauri::State<'_, AppState>,
    id: i64,
    kind: String,
) -> Result<crate::dto::ComposeDraft, String> {
    with_store(state.inner().clone(), move |store| {
        let h = store
            .header_by_id(id)
            .map_err(|_| "Couldn't open this message.".to_owned())?
            .ok_or_else(|| "That message is no longer here.".to_owned())?;
        let account_id = store
            .account_for_message(id)
            .map_err(|_| "Couldn't open this message.".to_owned())?
            .ok_or_else(|| "Couldn't open this message.".to_owned())?;
        let account = store
            .account_by_id(account_id)
            .map_err(|_| "Couldn't read the account.".to_owned())?
            .ok_or_else(|| "Couldn't read the account.".to_owned())?;
        let body = store.body_for(id).ok().flatten().unwrap_or_default();
        crate::dto::compose_draft_from(
            &h,
            body.plain.as_deref().unwrap_or_default(),
            account.display_name,
            account.email,
            &kind,
        )
    })
    .await
}

/// Send a message via the current account's SMTP server (S9.5). Runs on a worker (blocking +
/// network, P1); reuses the engine send path (Sent-save + threading). The account signature is
/// appended here — `run_send` does not add it (the Slint app appends in its UI layer too), so this
/// is the one place that must.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn send_message(
    state: tauri::State<'_, AppState>,
    account_id: i64,
    to: String,
    cc: String,
    subject: String,
    body: String,
    in_reply_to: Option<String>,
    references: Vec<String>,
    attachments: Vec<String>,
    markdown: bool,
    draft_id: Option<i64>,
) -> Result<(), String> {
    // Append the account's signature (SEND-7). Read it up front on the store thread.
    let signature = with_store(state.inner().clone(), move |store| {
        Ok(store
            .signature(account_id)
            .ok()
            .flatten()
            .unwrap_or_default())
    })
    .await?;
    let body = format!(
        "{body}{}",
        geleit_engine::message::signature_block(&signature)
    );

    let (db, secrets) = (state.db_path.clone(), state.secrets.clone());
    tauri::async_runtime::spawn_blocking(move || {
        let attachments = read_attachments(&attachments)?;
        geleit_engine::sync_actions::run_send(
            &db,
            &*secrets,
            account_id,
            &to,
            &cc,
            &subject,
            &body,
            in_reply_to,
            references,
            attachments,
            markdown,
            draft_id, // if this was a resumed draft, run_send deletes it after a successful send
        )
    })
    .await
    .map_err(|_| "The send task stopped unexpectedly.".to_owned())?
}

/// Save (or update) a local draft (SEND-5). Returns the draft's id so the composer can keep editing
/// the same row. The local copy is encrypted at rest and is always the source of truth. When the
/// opt-in **"Sync drafts to server"** setting is on (default off, SEND-5), a copy is also appended to
/// the account's Drafts folder so other mail clients see it — best-effort: a server failure never
/// fails the local save.
#[tauri::command]
pub async fn save_draft(
    state: tauri::State<'_, AppState>,
    account_id: i64,
    draft_id: Option<i64>,
    draft: ComposeDraft,
    attachments: Vec<String>,
) -> Result<i64, String> {
    let st = state.inner().clone();
    // Local save first (the source of truth), gathering what a server copy would need.
    let (id, plan) = with_store(st.clone(), move |store| {
        // Read the attachment files (blocking) on this worker thread; their bytes are stored with the
        // draft so a resumed draft keeps its files. Size-capped like the send path.
        let atts = read_draft_attachments(&attachments)?;
        let id = store
            .save_draft(account_id, draft_id, &draft_content_from(&draft))
            .map_err(|_| "Couldn't save the draft.".to_owned())?;
        store
            .replace_draft_attachments(id, &atts)
            .map_err(|_| "Couldn't save the draft's attachments.".to_owned())?;
        Ok((id, server_draft_plan(store, account_id, id, &draft, &atts)))
    })
    .await?;
    // Then, only if the opt-in setting is on and a Drafts folder exists, mirror it to the server.
    if let Some(plan) = plan {
        let (db, secrets) = (st.db_path.clone(), st.secrets.clone());
        let ServerDraftPlan {
            folder,
            message_id,
            bytes,
        } = plan;
        let (folder2, mid) = (folder.clone(), message_id);
        let synced = tauri::async_runtime::spawn_blocking(move || {
            geleit_engine::sync_actions::run_sync_draft(
                &db, &*secrets, account_id, &folder2, &mid, &bytes,
            )
        })
        .await
        .map_err(|_| "The draft task stopped unexpectedly.".to_owned())?;
        // Best-effort: the draft is saved locally either way. Only record the folder once the copy is
        // actually there — and on failure leave whatever was recorded ALONE, so an existing copy is
        // never forgotten (forgetting it would strand unsent content on the server for good).
        if synced.is_ok() {
            let _ = with_store(st, move |store| {
                store
                    .set_draft_server_folder(id, Some(folder.as_str()))
                    .map_err(|_| "Couldn't record the draft's server copy.".to_owned())
            })
            .await;
        }
    }
    Ok(id)
}

/// What a server copy of a draft needs: which folder, the stable Message-ID that identifies the copy,
/// and the RFC 5322 bytes.
struct ServerDraftPlan {
    folder: String,
    message_id: String,
    bytes: Vec<u8>,
}

/// Decide whether draft `id` should be mirrored to the server, and build what that needs. `None` when
/// the opt-in setting is off, the account has no Drafts folder, or the bytes can't be built — in all
/// of which cases the draft simply stays local (the default, privacy-preserving behaviour).
fn server_draft_plan(
    store: &Store,
    account_id: i64,
    id: i64,
    draft: &ComposeDraft,
    atts: &[geleit_store::DraftAttachment],
) -> Option<ServerDraftPlan> {
    if !sync_drafts_on(store) {
        return None;
    }
    let folder = drafts_folder(store, account_id)?;
    let account = store.account_by_id(account_id).ok()??;
    // The draft's own stored Message-ID. Never re-derive it from `id`: SQLite reuses the ids of
    // deleted drafts, and expunging by a re-derived id would destroy a stranded copy of the *dead*
    // draft that happened to share the number (migration 15).
    let message_id = store.draft_by_id(id).ok()??.msgid;
    let d = geleit_engine::message::Draft {
        from_name: account.display_name.clone(),
        from_addr: account.email,
        to: geleit_engine::sync_actions::parse_addrs(&draft.to),
        cc: geleit_engine::sync_actions::parse_addrs(&draft.cc),
        subject: draft.subject.clone(),
        body_text: draft.body.clone(),
        in_reply_to: draft.in_reply_to.clone(),
        references: draft.references.clone(),
        attachments: atts
            .iter()
            .map(|a| geleit_engine::message::Attachment {
                filename: a
                    .filename
                    .clone()
                    .unwrap_or_else(|| "attachment".to_owned()),
                content_type: a.content_type.clone(),
                data: a.data.clone(),
            })
            .collect(),
        html_body: None,
    };
    let bytes = geleit_engine::message::build_draft(&d, &message_id).ok()?;
    Some(ServerDraftPlan {
        folder,
        message_id,
        bytes,
    })
}

/// Whether the opt-in "sync drafts to the server" setting is on. Absent = **off** (the default).
fn sync_drafts_on(store: &Store) -> bool {
    store
        .get_setting(SYNC_DRAFTS_SETTING)
        .ok()
        .flatten()
        .is_some_and(|v| v == "1" || v == "true")
}

/// The account's Drafts folder, by name. `None` → the provider keeps none, so drafts live on this
/// device (and nothing is hidden from the rail). The choosing is pure — see
/// [`crate::dto::pick_drafts_folder`], which is the single answer to this question.
fn drafts_folder(store: &Store, account_id: i64) -> Option<String> {
    let names: Vec<String> = store
        .folders_for_account(account_id)
        .ok()?
        .into_iter()
        .map(|f| f.name)
        .collect();
    crate::dto::pick_drafts_folder(&names)
}

/// The opt-in setting key for mirroring drafts to the server's Drafts folder (default off, P2).
const SYNC_DRAFTS_SETTING: &str = "sync_drafts";

/// Read attachment files into draft-attachment rows (bytes + name/type). Reuses the send-path reader
/// and its 25 MB cap, then maps to the store's draft type.
fn read_draft_attachments(paths: &[String]) -> Result<Vec<geleit_store::DraftAttachment>, String> {
    Ok(read_attachments(paths)?
        .into_iter()
        .map(|a| geleit_store::DraftAttachment {
            filename: Some(a.filename),
            content_type: a.content_type,
            data: a.data,
        })
        .collect())
}

/// How many of the provider's drafts we list. A Drafts folder is a handful of messages; the cap only
/// stops a pathological one (a broken client that appended thousands) from stalling the pane.
const SERVER_DRAFTS_CAP: i64 = 200;

/// **One Drafts.** Every draft for an account, newest first — this device's *and* the ones in the
/// provider's Drafts folder (started in webmail, or on a phone).
///
/// The de-duplication is what makes this safe to merge: with "sync drafts" on, every local draft
/// already has a copy on the server, and listing both would show each one twice. `dto::merged_drafts`
/// folds our own copies (recognised by the `Message-ID` we stamped) back into the drafts they came
/// from. Purely local reads — the folder is *synced* by [`refresh_drafts`].
#[tauri::command]
pub async fn list_drafts(
    state: tauri::State<'_, AppState>,
    account_id: i64,
) -> Result<Vec<DraftSummary>, String> {
    with_store(state.inner().clone(), move |store| {
        let local = store
            .list_drafts(account_id)
            .map_err(|_| "Couldn't load your drafts.".to_owned())?;
        let server = server_drafts(store, account_id, local.len());
        Ok(crate::dto::merged_drafts(&local, &server))
    })
    .await
}

/// Read the provider's Drafts folder out of the **local store** (no network — [`refresh_drafts`] is
/// what fills it). Best-effort: a provider with no Drafts folder, or one we haven't synced yet, simply
/// contributes nothing, and the list is this device's drafts alone.
///
/// One store query for the whole folder (`drafts_in_folder`) — a Drafts pane that decrypted a body per
/// row just to ask "is this HTML?" would be a latency defect (P3).
fn server_drafts(
    store: &Store,
    account_id: i64,
    local_drafts: usize,
) -> Vec<crate::dto::ServerDraft> {
    let Some(name) = drafts_folder(store, account_id) else {
        return Vec::new(); // the provider keeps none — the drafts live here
    };
    let Some(folder) = store
        .folders_for_account(account_id)
        .unwrap_or_default()
        .into_iter()
        .find(|f| f.name == name)
    else {
        return Vec::new();
    };
    // Read past the cap by however many drafts we hold: with "sync drafts" on, our own copies are in
    // this folder too and every one of them is about to be deduped away. Capping *before* the dedup
    // would mean a user with 200 local drafts never sees the one they started in webmail — the exact
    // draft this feature exists for.
    let cap = SERVER_DRAFTS_CAP.saturating_add(i64::try_from(local_drafts).unwrap_or(i64::MAX));
    store
        .drafts_in_folder(folder.id, cap)
        .unwrap_or_default()
        .into_iter()
        .map(|d| crate::dto::ServerDraft {
            id: d.id,
            message_id: d.message_id,
            to: d.to_addrs.unwrap_or_default(),
            subject: d.subject.unwrap_or_default(),
            snippet: d.snippet.unwrap_or_default(),
            date: d.date.unwrap_or(0),
            formatted: d.formatted,
        })
        .collect()
}

/// Sync the provider's Drafts folder, so a draft started in webmail shows up here.
///
/// The scheduler only sweeps `INBOX`, and the Drafts folder is no longer selectable in the rail (the
/// Drafts entry *is* it), so nothing else would ever fetch it. Goes through [`sync_folder_once`] like
/// every other sync, so it takes the folder lock and can't race the scheduler or a Refresh.
///
/// Returns `false` when the provider keeps no Drafts folder — not an error: the drafts simply live on
/// this device.
#[tauri::command]
pub async fn refresh_drafts(
    state: tauri::State<'_, AppState>,
    account_id: i64,
) -> Result<bool, String> {
    let st = state.inner().clone();
    let folder = with_store(st.clone(), move |store| {
        Ok(drafts_folder(store, account_id))
    })
    .await?;
    let Some(folder) = folder else {
        return Ok(false);
    };
    sync_folder_once(&st, account_id, &folder).await?;
    Ok(true)
}

/// Continue a draft that lives in the provider's Drafts folder: load it back into the compose form,
/// attachments and all.
///
/// The text comes from the local store (already synced). The **attachments** are the one thing here
/// that needs the network — they're never stored on this device until they're needed (P2) — so they're
/// fetched now and written to temp files, exactly like a resumed local draft's are, so send and re-save
/// go down the ordinary path-based flow.
///
/// The attachments are **fail-closed**, unlike everywhere else we touch them: if any one of them can't
/// be fetched, the whole resume fails and the server copy stays untouched. Opening the draft without a
/// file and then expunging the original on save would destroy that file for good.
#[tauri::command]
pub async fn resume_server_draft(
    state: tauri::State<'_, AppState>,
    id: i64,
) -> Result<ResumedDraft, String> {
    let st = state.inner().clone();
    let plan = with_store(st.clone(), move |store| {
        let h = store
            .header_by_id(id)
            .map_err(|_| "Couldn't open that draft.".to_owned())?
            .ok_or_else(|| "That draft is no longer here.".to_owned())?;
        let body = store
            .body_for(id)
            .map_err(|_| "Couldn't open that draft.".to_owned())?;
        let loc = store
            .message_location(id)
            .map_err(|_| "Couldn't open that draft.".to_owned())?;
        let account_id = store
            .account_for_message(id)
            .map_err(|_| "Couldn't open that draft.".to_owned())?;
        let attachments = store.attachments_for(id).unwrap_or_default();
        Ok((h, body, loc, account_id, attachments))
    })
    .await?;
    let (h, body, loc, account_id, attachments) = plan;

    // Nothing to show means we must not open it: saving an empty compose form would replace the real
    // draft on the server with nothing. Two ways that can happen, and the guard has to cover both —
    // no body row at all (the folder hasn't finished downloading), and a body whose text is empty
    // while an HTML part exists (our text extraction gave us nothing, so the words are still in the
    // part we can't read). A draft that is genuinely blank — a subject and no text — opens fine.
    let text = body
        .as_ref()
        .and_then(|b| b.plain.clone())
        .unwrap_or_default();
    let has_html = body.as_ref().is_some_and(|b| b.html.is_some());
    if body.is_none() || (text.trim().is_empty() && has_html) {
        return Err("This draft hasn't finished downloading. Try again in a moment.".to_owned());
    }
    let draft = ComposeDraft {
        to: h.to_addrs.clone().unwrap_or_default(),
        cc: h.cc_addrs.clone().unwrap_or_default(),
        subject: h.subject.clone().unwrap_or_default(),
        body: text,
        // A half-written reply is still a reply: keep the threading headers so sending it lands in the
        // conversation it belongs to.
        in_reply_to: h.in_reply_to.clone(),
        references: h.in_reply_to.clone().into_iter().collect(),
    };

    // The message says it has files, but we hold no rows for them (a body stored before the attachment
    // table existed, or a sync that died between the two writes). Fetching "all zero" of them would open
    // the draft with no chips — and saving it would then expunge the original, taking the files with it.
    if h.has_attachments && attachments.is_empty() {
        return Err(
            "This draft's files haven't downloaded yet. Refresh, then try again.".to_owned(),
        );
    }

    let mut paths = Vec::new();
    if !attachments.is_empty() {
        let Some(((folder, uid), account_id)) = loc.zip(account_id) else {
            return Err("This draft's files can't be fetched (it isn't on a server).".to_owned());
        };
        let base = std::env::temp_dir().join(format!("geleit-server-draft-{id}"));
        for (i, a) in attachments.iter().enumerate() {
            let (db, secrets) = (st.db_path.clone(), st.secrets.clone());
            let (folder, uid) = (folder.clone(), uid as u32);
            let (fetched_name, bytes) = tauri::async_runtime::spawn_blocking(move || {
                geleit_engine::sync_actions::run_fetch_attachment(
                    &db, &*secrets, account_id, &folder, uid, i,
                )
            })
            .await
            .map_err(|_| "The download task stopped unexpectedly.".to_owned())?
            .map_err(|_| "Couldn't fetch this draft's files. Check your connection.".to_owned())?;

            let name = safe_attachment_filename(
                &fetched_name
                    .or_else(|| a.filename.clone())
                    .unwrap_or_else(|| format!("attachment-{}", i + 1)),
            );
            // Own sub-dir per file, so same-named files stay distinct while the basename (what the
            // composer chip shows) stays clean.
            let dir = base.join(i.to_string());
            std::fs::create_dir_all(&dir)
                .and_then(|()| {
                    let path = dir.join(&name);
                    std::fs::write(&path, &bytes).map(|()| path)
                })
                .map(|path| paths.push(path.to_string_lossy().into_owned()))
                .map_err(|_| "Couldn't open this draft's files.".to_owned())?;
        }
    }
    Ok(ResumedDraft {
        draft,
        attachments: paths,
    })
}

/// Load a draft's full content back into a compose form, to resume editing. `None` if it's gone. Its
/// saved attachments are materialised to temp files and their paths returned, so the composer can
/// send / re-save them through the normal path-based flow. Best-effort on attachments (a file that
/// can't be written is skipped rather than failing the resume).
#[tauri::command]
pub async fn load_draft(
    state: tauri::State<'_, AppState>,
    id: i64,
) -> Result<Option<ResumedDraft>, String> {
    with_store(state.inner().clone(), move |store| {
        let Some(row) = store
            .draft_by_id(id)
            .map_err(|_| "Couldn't open the draft.".to_owned())?
        else {
            return Ok(None);
        };
        let saved = store.draft_attachments(id).unwrap_or_default();
        let attachments = materialize_draft_attachments(id, &saved);
        Ok(Some(ResumedDraft {
            draft: compose_from_draft(row.content),
            attachments,
        }))
    })
    .await
}

/// Write a draft's saved attachments to a per-draft temp dir so the composer can send / re-save them
/// through the normal path-based flow; returns the paths written. Best-effort — a file that can't be
/// written is skipped. Each attachment gets its own numbered sub-dir so same-named files stay distinct
/// while the **basename stays clean** (what the composer chip shows); the name is sanitised so a
/// hostile stored filename can't escape the temp dir.
fn materialize_draft_attachments(
    draft_id: i64,
    atts: &[geleit_store::DraftAttachment],
) -> Vec<String> {
    let base = std::env::temp_dir().join(format!("geleit-draft-{draft_id}"));
    let mut paths = Vec::new();
    for (i, a) in atts.iter().enumerate() {
        let name = a
            .filename
            .as_deref()
            .map(safe_attachment_filename)
            .unwrap_or_else(|| format!("attachment-{}", i + 1));
        let dir = base.join(i.to_string());
        if std::fs::create_dir_all(&dir).is_err() {
            continue;
        }
        let path = dir.join(&name);
        if std::fs::write(&path, &a.data).is_ok() {
            paths.push(path.to_string_lossy().into_owned());
        }
    }
    paths
}

/// Delete a saved draft (idempotent). Used by the draft-list delete affordance.
#[tauri::command]
pub async fn delete_draft(state: tauri::State<'_, AppState>, id: i64) -> Result<(), String> {
    let st = state.inner().clone();
    // Note where any server copy lives (opt-in "sync drafts") before dropping the row that records it.
    let server = with_store(st.clone(), move |store| {
        // Read the row's own Message-ID and server folder before deleting it — both are gone after.
        let row = store.draft_by_id(id).ok().flatten();
        let account = store.account_for_draft(id).ok().flatten();
        let mid = row.as_ref().map(|r| r.msgid.clone());
        let folder = row.and_then(|r| r.server_folder);
        store
            .delete_draft(id)
            .map_err(|_| "Couldn't delete the draft.".to_owned())?;
        // The mirrored copy is a *message* row too, once the Drafts folder has been synced. Drop it
        // with the draft: the merged list folds our copies into the drafts we hold, so a copy whose
        // draft is gone stops being folded — and the draft the user just deleted would come back as an
        // "On your provider" row, still resumable, until a sync happened to reconcile it away.
        if let (Some(account_id), Some(mid)) = (account, mid.as_deref()) {
            let _ = store.delete_message_by_message_id(account_id, mid);
        }
        Ok(folder.zip(account).zip(mid))
    })
    .await?;
    // Best-effort: the local draft is already gone; a failed server cleanup must not error the UI.
    if let Some(((folder, account_id), mid)) = server {
        let (db, secrets) = (st.db_path.clone(), st.secrets.clone());
        let _ = tauri::async_runtime::spawn_blocking(move || {
            geleit_engine::sync_actions::run_expunge_server_draft(
                &db, &*secrets, account_id, &folder, &mid,
            )
        })
        .await;
    }
    Ok(())
}

/// Sweep every server copy of this account's drafts away (SEND-5) — called when the opt-in "sync
/// drafts" setting is switched **off**, so turning it off actually takes the drafts back off the
/// server rather than just stopping new uploads. Best-effort per draft; the local drafts are
/// untouched.
#[tauri::command]
pub async fn purge_server_drafts(
    state: tauri::State<'_, AppState>,
    account_id: i64,
) -> Result<(), String> {
    let st = state.inner().clone();
    let copies = with_store(st.clone(), move |store| {
        store
            .drafts_with_server_copies(account_id)
            .map_err(|_| "Couldn't read your drafts.".to_owned())
    })
    .await?;
    for (draft_id, folder, mid) in copies {
        let (db, secrets) = (st.db_path.clone(), st.secrets.clone());
        let gone = tauri::async_runtime::spawn_blocking(move || {
            geleit_engine::sync_actions::run_expunge_server_draft(
                &db, &*secrets, account_id, &folder, &mid,
            )
        })
        .await
        .map_err(|_| "The draft task stopped unexpectedly.".to_owned())?;
        // Only forget the copy once it's actually gone — otherwise it would be stranded there.
        if gone.is_ok() {
            let _ = with_store(st.clone(), move |store| {
                store
                    .set_draft_server_folder(draft_id, None)
                    .map_err(|_| "Couldn't update the draft.".to_owned())
            })
            .await;
        }
    }
    Ok(())
}

/// Distinct past-sender addresses matching a prefix, for To/Cc autocomplete (SEND-9). Read-only;
/// capped small so the dropdown stays calm. Empty prefix returns nothing (the store handles that).
#[tauri::command]
pub async fn suggest_addresses(
    state: tauri::State<'_, AppState>,
    account_id: i64,
    prefix: String,
) -> Result<Vec<String>, String> {
    with_store(state.inner().clone(), move |store| {
        store
            .suggest_addresses(account_id, &prefix, 6)
            .map_err(|_| "Couldn't look up addresses.".to_owned())
    })
    .await
}

/// Read attachment files from disk into message attachments, guessing each content type from its
/// name. Total size is capped so a stray huge file fails calmly instead of choking the SMTP server.
///
/// The paths come from the trusted app frontend (the picker), not from mail content — hostile mail
/// is confined to the sandboxed `mail://` iframe (no scripts, no same-origin) and can't reach IPC.
fn read_attachments(paths: &[String]) -> Result<Vec<geleit_engine::message::Attachment>, String> {
    const MAX_TOTAL: u64 = 25 * 1024 * 1024; // 25 MB — a common provider ceiling
    let mut total: u64 = 0;
    let mut out = Vec::with_capacity(paths.len());
    for p in paths {
        // Check the size *before* reading, so an enormous file is rejected rather than pulled into
        // memory whole (which would defeat the cap).
        total += std::fs::metadata(p)
            .map_err(|_| "Couldn't read an attachment file.".to_owned())?
            .len();
        if total > MAX_TOTAL {
            return Err("Attachments are too large to send (25 MB max).".to_owned());
        }
        let data = std::fs::read(p).map_err(|_| "Couldn't read an attachment file.".to_owned())?;
        let filename = std::path::Path::new(p)
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_else(|| "attachment".to_owned());
        let content_type = geleit_engine::message::guess_content_type(&filename);
        out.push(geleit_engine::message::Attachment {
            filename,
            content_type,
            data,
        });
    }
    Ok(out)
}

/// Open a native file picker (zenity, then kdialog) and return the chosen paths. Runs the desktop's
/// own dialog as a subprocess — deliberately not an in-process GTK dialog, which clashes with the
/// webview's event loop. Empty result = the user cancelled.
#[tauri::command]
pub async fn pick_files() -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        // zenity: multiple selection, newline-separated. A newline in a filename (pathological on
        // desktop, but legal on Linux) would mis-split — an acceptable edge for a file picker.
        match std::process::Command::new("zenity")
            .args([
                "--file-selection",
                "--multiple",
                "--separator=\n",
                "--title=Attach files",
            ])
            .output()
        {
            Ok(o) if o.status.success() => {
                return Ok(String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .filter(|l| !l.is_empty())
                    .map(str::to_owned)
                    .collect());
            }
            Ok(_) => return Ok(Vec::new()), // ran but the user cancelled
            Err(_) => {}                    // zenity not installed — try kdialog
        }
        // kdialog fallback: single file (its multi-select output isn't newline-safe).
        match std::process::Command::new("kdialog")
            .args(["--getopenfilename", "."])
            .output()
        {
            Ok(o) if o.status.success() => {
                let p = String::from_utf8_lossy(&o.stdout).trim().to_owned();
                Ok(if p.is_empty() { Vec::new() } else { vec![p] })
            }
            Ok(_) => Ok(Vec::new()),
            Err(_) => Err("No file picker found — install zenity or kdialog.".to_owned()),
        }
    })
    .await
    .map_err(|_| "The file picker stopped unexpectedly.".to_owned())?
}

/// A native "save as" dialog, pre-filled with `default_name`. `Ok(Some(path))` = chosen,
/// `Ok(None)` = the user cancelled, `Err` = no dialog tool is installed (so the caller can say so,
/// rather than silently no-op). Blocking — call inside `spawn_blocking`.
fn pick_save_path(default_name: &str) -> Result<Option<String>, String> {
    let attempts: [(&str, Vec<String>); 2] = [
        (
            "zenity",
            vec![
                "--file-selection".into(),
                "--save".into(),
                "--confirm-overwrite".into(),
                "--title=Save message".into(),
                format!("--filename={default_name}"),
            ],
        ),
        (
            "kdialog",
            vec!["--getsavefilename".into(), format!("./{default_name}")],
        ),
    ];
    for (cmd, args) in attempts {
        match std::process::Command::new(cmd).args(&args).output() {
            Ok(out) if out.status.success() => {
                let path = String::from_utf8_lossy(&out.stdout).trim().to_owned();
                return Ok((!path.is_empty()).then_some(path));
            }
            Ok(_) => return Ok(None), // ran but cancelled
            Err(_) => continue,       // not installed → try the next tool
        }
    }
    Err("No file picker found — install zenity or kdialog.".to_owned())
}

/// Save an open message to disk as a `.eml` file (READ-10). Rebuilds RFC 822 bytes from what's stored
/// (no network), asks where to save via a native dialog, and writes. Returns whether a file was
/// written (`false` = the user cancelled the dialog).
#[tauri::command]
pub async fn save_eml(state: tauri::State<'_, AppState>, id: i64) -> Result<bool, String> {
    // Read the header + body and build the bytes on the store thread.
    let (bytes, default_name) = with_store(state.inner().clone(), move |store| {
        let header = store
            .header_by_id(id)
            .map_err(|_| "Couldn't load the message to save.".to_owned())?
            .ok_or_else(|| "That message is no longer here.".to_owned())?;
        let body = store.body_for(id).ok().flatten();
        let bytes = geleit_engine::message::export_eml(&header, body.as_ref())?;
        let name = format!(
            "{}.eml",
            safe_filename_stem(header.subject.as_deref().unwrap_or("message"))
        );
        Ok((bytes, name))
    })
    .await?;
    tauri::async_runtime::spawn_blocking(move || {
        let Some(path) = pick_save_path(&default_name)? else {
            return Ok(false); // cancelled
        };
        std::fs::write(&path, &bytes)
            .map(|()| true)
            .map_err(|_| "Couldn't write that file.".to_owned())
    })
    .await
    .map_err(|_| "The save task stopped unexpectedly.".to_owned())?
}

/// Save a message's `index`-th attachment to disk (READ-8). The bytes aren't stored locally, so this
/// fetches the raw message from the server on demand (`BODY.PEEK[]`), extracts the part, and writes
/// it via a native save dialog. Returns whether a file was written (`false` = cancelled).
#[tauri::command]
pub async fn save_attachment(
    state: tauri::State<'_, AppState>,
    message_id: i64,
    index: usize,
) -> Result<bool, String> {
    let st = state.inner().clone();
    // Resolve the server location + account and the stored default name on the store thread.
    let plan = with_store(st.clone(), move |store| {
        let loc = store
            .message_location(message_id)
            .map_err(|_| "Couldn't save the attachment.".to_owned())?;
        let acc = store
            .account_for_message(message_id)
            .map_err(|_| "Couldn't save the attachment.".to_owned())?;
        let meta_name = store
            .attachments_for(message_id)
            .ok()
            .and_then(|a| a.into_iter().nth(index))
            .and_then(|a| a.filename);
        Ok(loc
            .zip(acc)
            .map(|((folder, uid), account)| (folder, uid, account, meta_name)))
    })
    .await?;
    let Some((folder, uid, account_id, meta_name)) = plan else {
        return Err("This attachment can't be saved (the message isn't on a server).".to_owned());
    };
    // Fetch the raw message and extract the part on a worker (network + parse).
    let (db, secrets) = (st.db_path.clone(), st.secrets.clone());
    let (fetched_name, bytes) = tauri::async_runtime::spawn_blocking(move || {
        geleit_engine::sync_actions::run_fetch_attachment(
            &db, &*secrets, account_id, &folder, uid as u32, index,
        )
    })
    .await
    .map_err(|_| "The download task stopped unexpectedly.".to_owned())??;
    let default_name = safe_attachment_filename(
        &fetched_name
            .or(meta_name)
            .unwrap_or_else(|| format!("attachment-{}", index + 1)),
    );
    tauri::async_runtime::spawn_blocking(move || {
        let Some(path) = pick_save_path(&default_name)? else {
            return Ok(false); // cancelled
        };
        std::fs::write(&path, &bytes)
            .map(|()| true)
            .map_err(|_| "Couldn't write that file.".to_owned())
    })
    .await
    .map_err(|_| "The save task stopped unexpectedly.".to_owned())?
}

/// Open a `.eml` file from disk (READ-10): pick a file, parse it, store it in the account's local
/// **Saved** folder, and return the new message id so the UI can switch there and open it. Returns
/// `None` if the user cancelled. No network — the file is parsed and rendered like any synced mail.
#[tauri::command]
pub async fn open_eml_file(
    state: tauri::State<'_, AppState>,
    account_id: i64,
) -> Result<Option<i64>, String> {
    // Pick + read the file off the async runtime (dialog + disk are blocking).
    let bytes = tauri::async_runtime::spawn_blocking(pick_open_eml)
        .await
        .map_err(|_| "The file picker stopped unexpectedly.".to_owned())??;
    let Some(bytes) = bytes else {
        return Ok(None); // cancelled
    };
    with_store(state.inner().clone(), move |store| {
        let eml = geleit_engine::message::parse_eml(&bytes);
        let folder = store
            .upsert_folder(account_id, geleit_store::SAVED_FOLDER)
            .map_err(|_| "Couldn't open the file.".to_owned())?;
        let snippet: Option<String> = eml.plain.as_deref().map(|t| t.chars().take(140).collect());
        let new = geleit_store::NewMessage {
            uid: None, // a local-only message — no server UID
            message_id: eml.message_id,
            subject: eml.subject,
            from_name: eml.from_name,
            from_addr: eml.from_addr,
            to_addrs: eml.to_addrs,
            date: eml.date,
            has_attachments: eml.has_attachments,
            snippet: snippet.clone(),
            ..Default::default()
        };
        let id = store
            .upsert_message(account_id, folder, &new)
            .map_err(|_| "Couldn't import the message.".to_owned())?;
        store
            .store_body(
                id,
                eml.plain.as_deref(),
                eml.html.as_deref(),
                snippet.as_deref(),
                eml.has_attachments,
            )
            .map_err(|_| "Couldn't import the message.".to_owned())?;
        Ok(Some(id))
    })
    .await
}

/// Pick a single `.eml`/message file and read its bytes. Returns `None` if cancelled. Blocking.
fn pick_open_eml() -> Result<Option<Vec<u8>>, String> {
    let path = {
        let zen = std::process::Command::new("zenity")
            .args(["--file-selection", "--title=Open mail file"])
            .output();
        match zen {
            Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_owned(),
            Ok(_) => return Ok(None), // cancelled
            Err(_) => {
                // zenity missing — try kdialog.
                match std::process::Command::new("kdialog")
                    .args(["--getopenfilename", "."])
                    .output()
                {
                    Ok(o) if o.status.success() => {
                        String::from_utf8_lossy(&o.stdout).trim().to_owned()
                    }
                    Ok(_) => return Ok(None),
                    Err(_) => {
                        return Err("No file picker found — install zenity or kdialog.".to_owned())
                    }
                }
            }
        }
    };
    if path.is_empty() {
        return Ok(None);
    }
    std::fs::read(&path)
        .map(Some)
        .map_err(|_| "Couldn't read that file.".to_owned())
}

/// Every account's id, for the background scheduler's sweep.
pub(crate) async fn account_ids(state: &AppState) -> Result<Vec<i64>, String> {
    with_store(state.clone(), |store| {
        store
            .list_accounts()
            .map(|accounts| accounts.into_iter().map(|a| a.id).collect())
            .map_err(|_| "Couldn't read your accounts.".to_owned())
    })
    .await
}

/// Sync one folder's recent mail — the single path **both** a user-pressed Refresh and the background
/// scheduler go through, so the folder's sync lock is honoured in one place. Returns what arrived
/// (slice 3 turns that into a notification).
pub(crate) async fn sync_folder_once(
    state: &AppState,
    account_id: i64,
    folder: &str,
) -> Result<geleit_engine::imap::SyncOutcome, String> {
    let lock = state.sync_lock(account_id, folder);
    let _held = lock.lock().await; // queue behind any sync of this folder already in flight
    let (db, secrets, folder) = (
        state.db_path.clone(),
        state.secrets.clone(),
        folder.to_owned(),
    );
    tauri::async_runtime::spawn_blocking(move || {
        geleit_engine::sync_actions::run_refresh(&db, &*secrets, account_id, &folder)
    })
    .await
    .map_err(|_| "The sync task stopped unexpectedly.".to_owned())?
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
    let st = state.inner().clone();
    let (db, secrets) = (state.db_path.clone(), state.secrets.clone());

    // Phase 1 — recent mail. Await this so the caller can re-list once it's in. Behind the folder's
    // sync lock, so a background sync of the same folder can't run alongside it (see `sync_lock`);
    // if one is already running, this waits for it and then syncs again — finding nothing new, which
    // is exactly right.
    sync_folder_once(&st, account_id, &folder).await?;
    // That worked, so we're online — which is the one thing the background scheduler can't know while
    // it sits in a backed-off sleep. Wake it: it resets and sweeps the other accounts at once, rather
    // than leaving their mail up to half an hour stale after a laptop comes back from a night off.
    st.wake_sync().notify_waiters();

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

/// Dev/test seam, debug builds only: `GELEIT_COMPOSE=new|reply|reply_all|forward` opens the compose
/// overlay on boot so it can be screenshot-verified without a click. Never in release.
#[cfg(debug_assertions)]
#[tauri::command]
pub async fn dev_compose() -> Option<String> {
    std::env::var("GELEIT_COMPOSE").ok()
}

/// Dev/test seam, debug builds only: `GELEIT_UNIFIED=1` opens the merged "All inboxes" view on boot
/// so it can be screenshot-verified without a click. Never in release.
#[cfg(debug_assertions)]
#[tauri::command]
pub async fn dev_unified() -> bool {
    std::env::var("GELEIT_UNIFIED").is_ok_and(|v| v == "1")
}

/// Dev/test seam, debug builds only: `GELEIT_SETUP=1` opens the add-account overlay on boot. Never
/// in release.
#[cfg(debug_assertions)]
#[tauri::command]
pub async fn dev_setup() -> bool {
    std::env::var("GELEIT_SETUP").is_ok_and(|v| v == "1")
}

/// Dev/test seam, debug builds only: `GELEIT_SETTINGS=1` opens the Settings window on boot, or
/// `GELEIT_SETTINGS=<tab>` (accounts|general|appearance|privacy|notifications) opens it on that tab.
/// Never in release.
#[cfg(debug_assertions)]
#[tauri::command]
pub async fn dev_settings() -> Option<String> {
    const TABS: [&str; 6] = [
        "1",
        "accounts",
        "general",
        "appearance",
        "privacy",
        "notifications",
    ];
    std::env::var("GELEIT_SETTINGS")
        .ok()
        .filter(|v| TABS.contains(&v.as_str()))
}

/// Dev/test seam, debug builds only: `GELEIT_SEARCH=<query>` opens search and runs it on boot. Never
/// in release.
#[cfg(debug_assertions)]
#[tauri::command]
pub async fn dev_search() -> Option<String> {
    std::env::var("GELEIT_SEARCH").ok()
}

/// Dev/test seam, debug builds only: `GELEIT_TRASH=empty|delete` opens the irreversible-delete confirm
/// dialog on boot (there's no click injection for the danger dialogs otherwise). Never in release.
#[cfg(debug_assertions)]
#[tauri::command]
pub async fn dev_trash() -> Option<String> {
    std::env::var("GELEIT_TRASH").ok()
}

/// Dev/test seam, debug builds only: with `GELEIT_COMPOSE=new`, `GELEIT_TO=<text>` pre-fills the To
/// input on boot so the address-autocomplete dropdown can be screenshotted. Never in release.
#[cfg(debug_assertions)]
#[tauri::command]
pub async fn dev_compose_to() -> Option<String> {
    std::env::var("GELEIT_TO").ok()
}

/// Dev/test seam, debug builds only: `GELEIT_DRAFTS=1` opens the Drafts list on boot. Never in release.
#[cfg(debug_assertions)]
#[tauri::command]
pub async fn dev_drafts() -> bool {
    std::env::var("GELEIT_DRAFTS").is_ok_and(|v| v == "1")
}

/// Dev/test seam, debug builds only: `GELEIT_RESUME=1` resumes the newest draft on boot (opens the
/// composer with its content + materialised attachments). Never in release.
#[cfg(debug_assertions)]
#[tauri::command]
pub async fn dev_resume() -> bool {
    std::env::var("GELEIT_RESUME").is_ok_and(|v| v == "1")
}

/// Dev/test seam, debug builds only: `GELEIT_SELECT=<id,id,…>` pre-selects those message rows on boot
/// so the multi-select bulk bar can be screenshotted. Never in release.
#[cfg(debug_assertions)]
#[tauri::command]
pub async fn dev_select() -> Option<String> {
    std::env::var("GELEIT_SELECT").ok()
}

/// Dev/test seam, debug builds only: `GELEIT_FOLDER=new` opens the New-folder dialog on boot;
/// `GELEIT_FOLDER=menu` opens the first user folder's ⋯ (Rename/Delete) menu. Never in release.
#[cfg(debug_assertions)]
#[tauri::command]
pub async fn dev_folder() -> Option<String> {
    std::env::var("GELEIT_FOLDER").ok()
}

#[cfg(test)]
mod tests {
    use super::{materialize_draft_attachments, read_attachments, server_drafts, AppState};
    use geleit_platform::secret::InMemorySecretStore;
    use geleit_store::{DraftContent, NewMessage, Store};

    /// The seam where the real data shapes meet the pure merge: a **namespaced** Drafts folder, read
    /// out of a real store, folded against the drafts we hold. Everything the pure tests can't see —
    /// the folder round-trip, the columns, the id spaces — lives here.
    #[test]
    fn the_drafts_list_merges_the_providers_drafts_folder_out_of_the_store() {
        let s = Store::open_in_memory().unwrap();
        let acc = s.add_account("me@example.com", None).unwrap();
        s.upsert_folder(acc, "INBOX").unwrap();
        let drafts = s.upsert_folder(acc, "INBOX.Drafts").unwrap();

        // A draft of ours, mirrored to the server ("sync drafts" on) — appended under its own id.
        let mine = s
            .save_draft(
                acc,
                None,
                &DraftContent {
                    subject: "Mine".to_owned(),
                    ..Default::default()
                },
            )
            .unwrap();
        let my_msgid = s.draft_by_id(mine).unwrap().unwrap().msgid;
        s.upsert_message(
            acc,
            drafts,
            &NewMessage {
                uid: Some(1),
                message_id: Some(my_msgid),
                subject: Some("Mine".to_owned()),
                date: Some(100),
                ..Default::default()
            },
        )
        .unwrap();

        // …and one started in webmail: formatted, addressed to someone, not ours.
        let theirs = s
            .upsert_message(
                acc,
                drafts,
                &NewMessage {
                    uid: Some(2),
                    message_id: Some("<written-in-webmail@example.org>".to_owned()),
                    to_addrs: Some("hazel@example.org".to_owned()),
                    subject: Some("The roof".to_owned()),
                    date: Some(200),
                    ..Default::default()
                },
            )
            .unwrap();
        s.store_body(
            theirs,
            Some("the words"),
            Some("<p>the words</p>"),
            None,
            false,
        )
        .unwrap();

        let local = s.list_drafts(acc).unwrap();
        let server = server_drafts(&s, acc, local.len());
        let rows = crate::dto::merged_drafts(&local, &server);

        assert_eq!(
            rows.len(),
            2,
            "our own copy folded back into the draft it is"
        );
        let webmail = rows
            .iter()
            .find(|r| r.on_server)
            .expect("the webmail draft");
        assert_eq!(webmail.id, theirs, "a server row carries the MESSAGE id");
        assert_eq!(webmail.to, "hazel@example.org");
        assert_eq!(webmail.subject, "The roof");
        assert!(
            webmail.formatted,
            "it has an HTML part — warn before replacing it"
        );

        let ours = rows.iter().find(|r| !r.on_server).expect("our draft");
        assert_eq!(ours.id, mine, "a local row carries the DRAFT id");
        assert!(!ours.formatted);
    }

    /// A provider that keeps no Drafts folder: the drafts live on this device, and nothing is hidden.
    #[test]
    fn with_no_drafts_folder_on_the_provider_the_list_is_this_devices_drafts_alone() {
        let s = Store::open_in_memory().unwrap();
        let acc = s.add_account("me@example.com", None).unwrap();
        s.upsert_folder(acc, "INBOX").unwrap();
        s.save_draft(acc, None, &DraftContent::default()).unwrap();

        assert!(server_drafts(&s, acc, 1).is_empty());
        let rows = crate::dto::merged_drafts(&s.list_drafts(acc).unwrap(), &[]);
        assert_eq!(rows.len(), 1);
        assert!(!rows[0].on_server);
    }

    #[test]
    fn one_sync_lock_per_folder_shared_by_every_syncer() {
        // The guard that keeps the background scheduler and a user-pressed Refresh from syncing one
        // folder at once (which would fetch the same mail twice, and notify twice for one email).
        // Two callers asking for the same folder must get the SAME lock; different folders must not
        // block each other.
        let st = AppState::new(
            ":memory:".to_owned(),
            std::sync::Arc::new(InMemorySecretStore::new()),
        );

        let a1 = st.sync_lock(1, "INBOX");
        let a2 = st.sync_lock(1, "INBOX"); // the scheduler and Refresh, same folder
        assert!(
            std::sync::Arc::ptr_eq(&a1, &a2),
            "the same folder must share one lock, or two syncs could run over each other"
        );

        // A different folder — and a different account's same-named folder — are independent, so
        // syncing two accounts doesn't serialize them behind each other.
        assert!(!std::sync::Arc::ptr_eq(&a1, &st.sync_lock(1, "Archive")));
        assert!(!std::sync::Arc::ptr_eq(&a1, &st.sync_lock(2, "INBOX")));

        // Holding one folder's lock leaves the others free.
        let held = a1.try_lock().expect("free");
        assert!(
            st.sync_lock(1, "INBOX").try_lock().is_err(),
            "same folder is busy"
        );
        assert!(
            st.sync_lock(2, "INBOX").try_lock().is_ok(),
            "other account is free"
        );
        drop(held);
        assert!(st.sync_lock(1, "INBOX").try_lock().is_ok(), "released");
    }
    use geleit_store::DraftAttachment;

    #[test]
    fn materialize_draft_attachments_writes_files_and_returns_paths() {
        let atts = vec![
            DraftAttachment {
                filename: Some("notes.txt".to_owned()),
                content_type: "text/plain".to_owned(),
                data: b"hello".to_vec(),
            },
            // A hostile / missing name: must be sanitised, never escape the temp dir.
            DraftAttachment {
                filename: Some("../../escape.txt".to_owned()),
                content_type: "text/plain".to_owned(),
                data: b"world".to_vec(),
            },
            DraftAttachment {
                filename: None,
                content_type: "application/octet-stream".to_owned(),
                data: b"anon".to_vec(),
            },
        ];
        // Use a process-unique draft id so parallel test runs don't collide.
        let draft_id = 900_000 + (std::process::id() as i64 % 1000);
        let paths = materialize_draft_attachments(draft_id, &atts);
        assert_eq!(paths.len(), 3);
        // Bytes round-trip, and every path stays inside the per-draft temp dir (no traversal).
        let dir = std::env::temp_dir().join(format!("geleit-draft-{draft_id}"));
        assert_eq!(std::fs::read(&paths[0]).unwrap(), b"hello");
        assert_eq!(std::fs::read(&paths[1]).unwrap(), b"world");
        assert_eq!(std::fs::read(&paths[2]).unwrap(), b"anon");
        for p in &paths {
            assert!(std::path::Path::new(p).starts_with(&dir), "escaped: {p}");
        }
        // No attachments → no paths (and no dir work).
        assert!(materialize_draft_attachments(draft_id, &[]).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_attachments_reads_name_type_and_bytes() {
        let dir = std::env::temp_dir().join(format!("geleit-att-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("report.pdf");
        std::fs::write(&f, b"hello").unwrap();

        let atts = read_attachments(&[f.to_string_lossy().into_owned()]).unwrap();
        assert_eq!(atts.len(), 1);
        assert_eq!(atts[0].filename, "report.pdf");
        assert_eq!(atts[0].content_type, "application/pdf");
        assert_eq!(atts[0].data, b"hello");

        // a path that doesn't exist is a calm error, not a panic
        assert!(read_attachments(&["/no/such/geleit/file".to_owned()]).is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
