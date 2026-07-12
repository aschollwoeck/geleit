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
    compose_from_draft, display_sender, display_subject, draft_content_from, draft_summary,
    folder_rank, AccountDto, ComposeDraft, DraftSummary, FolderDto, MessageBodyDto, MessageDto,
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
/// the same row. Local-only and encrypted at rest; never touches the server.
#[tauri::command]
pub async fn save_draft(
    state: tauri::State<'_, AppState>,
    account_id: i64,
    draft_id: Option<i64>,
    draft: ComposeDraft,
) -> Result<i64, String> {
    with_store(state.inner().clone(), move |store| {
        store
            .save_draft(account_id, draft_id, &draft_content_from(&draft))
            .map_err(|_| "Couldn't save the draft.".to_owned())
    })
    .await
}

/// Every saved draft for an account, newest first, as list summaries.
#[tauri::command]
pub async fn list_drafts(
    state: tauri::State<'_, AppState>,
    account_id: i64,
) -> Result<Vec<DraftSummary>, String> {
    with_store(state.inner().clone(), move |store| {
        store
            .list_drafts(account_id)
            .map(|rows| rows.iter().map(draft_summary).collect())
            .map_err(|_| "Couldn't load your drafts.".to_owned())
    })
    .await
}

/// Load a draft's full content back into a compose form, to resume editing. `None` if it's gone.
#[tauri::command]
pub async fn load_draft(
    state: tauri::State<'_, AppState>,
    id: i64,
) -> Result<Option<ComposeDraft>, String> {
    with_store(state.inner().clone(), move |store| {
        store
            .draft_by_id(id)
            .map(|row| row.map(|r| compose_from_draft(r.content)))
            .map_err(|_| "Couldn't open the draft.".to_owned())
    })
    .await
}

/// Delete a saved draft (idempotent). Used by the draft-list delete affordance.
#[tauri::command]
pub async fn delete_draft(state: tauri::State<'_, AppState>, id: i64) -> Result<(), String> {
    with_store(state.inner().clone(), move |store| {
        store
            .delete_draft(id)
            .map_err(|_| "Couldn't delete the draft.".to_owned())
    })
    .await
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

/// Dev/test seam, debug builds only: `GELEIT_SETTINGS=1` opens the Settings window on boot. Never in
/// release.
#[cfg(debug_assertions)]
#[tauri::command]
pub async fn dev_settings() -> bool {
    std::env::var("GELEIT_SETTINGS").is_ok_and(|v| v == "1")
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

#[cfg(test)]
mod tests {
    use super::read_attachments;

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
