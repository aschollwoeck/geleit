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
use crate::shell::Shell;

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
    /// Where new-mail notifications go. Injected (like [`Self::secrets`]) so the tests never need a
    /// desktop, and so the app never talks to D-Bus directly.
    pub notifier: Arc<dyn geleit_platform::notify::Notifier>,
    /// Single-flight for the flag-write-back flush, per account: the value is "run once more when the
    /// current flush finishes". Without it, a bulk mark-read (one `set_read` per message) would spawn a
    /// flush thread *per message*, each pushing the whole account queue — a thundering herd. See
    /// [`spawn_flush`].
    flushing: Arc<Mutex<HashMap<i64, bool>>>,
    /// Single-flight for the outbox drain, per account. A scheduler sweep and a Refresh can both drain
    /// at once; without this they'd read the same pending rows and **send each message twice** (SMTP
    /// isn't idempotent). The value is "run once more when the current drain finishes" — so a Retry
    /// made *during* a drain (which clears a row's `failed` after the drain read its pending set) is
    /// still picked up by that drain's rerun, not deferred to the next sweep.
    draining_outbox: Arc<Mutex<HashMap<i64, bool>>>,
    /// Single-flight for the offline-move flush, per account (OFF-4) — same shape and reason as
    /// `draining_outbox`. A scheduler sweep and a fresh move can both flush at once; without this they'd
    /// read the same `pending_move` rows and run every move twice (a wasted round-trip, and a second
    /// `move_message` for a message already gone from the source folder is an error, not a no-op).
    draining_moves: Arc<Mutex<HashMap<i64, bool>>>,
    /// The accounts that already have a live IMAP IDLE watcher (instant new-mail push), each mapped to a
    /// **cancel signal** for its watcher. Populated by the host's `idle` worker at startup and when an account is
    /// added, so a just-added account gets IDLE at once instead of only after the next launch — and so
    /// the same account is never watched twice. Removing an account fires its cancel so the watcher stops
    /// promptly (rather than lingering on an authenticated connection) and frees the slot — which matters
    /// because SQLite reuses a removed account's id, and the next account added could take it.
    idle_watchers: Arc<Mutex<HashMap<i64, Arc<tokio::sync::Notify>>>>,
    /// Single-flight for the progressive backfill, per `(account, folder)`. The host's background
    /// `backfill` worker and a user-pressed Refresh both drive `run_backfill` for a folder;
    /// without this they'd each compute the same missing-UID set and download it twice. Whoever claims
    /// the pair backfills it; the other skips (the work still gets done).
    backfilling: Arc<Mutex<std::collections::HashSet<(i64, String)>>>,
}

impl AppState {
    pub fn new(db_path: String, secrets: Arc<dyn SecretStore>) -> Self {
        Self::with_notifier(
            db_path,
            secrets,
            Arc::new(geleit_platform::os_notify::DesktopNotifier::new()),
        )
    }

    /// The same, with the notifier chosen by the caller (tests use the fake — no desktop needed).
    #[must_use]
    pub fn with_notifier(
        db_path: String,
        secrets: Arc<dyn SecretStore>,
        notifier: Arc<dyn geleit_platform::notify::Notifier>,
    ) -> Self {
        Self {
            db_path,
            secrets,
            store: Arc::new(Mutex::new(None)),
            sync_locks: Arc::new(Mutex::new(HashMap::new())),
            wake_sync: Arc::new(tokio::sync::Notify::new()),
            notifier,
            flushing: Arc::new(Mutex::new(HashMap::new())),
            draining_outbox: Arc::new(Mutex::new(HashMap::new())),
            draining_moves: Arc::new(Mutex::new(HashMap::new())),
            idle_watchers: Arc::new(Mutex::new(HashMap::new())),
            backfilling: Arc::new(Mutex::new(std::collections::HashSet::new())),
        }
    }

    /// Claim the backfill of `(account_id, folder)`: `true` if this caller should do it, `false` if
    /// another backfill of the same folder is already running. Pair with [`Self::end_backfill`].
    pub fn try_begin_backfill(&self, account_id: i64, folder: &str) -> bool {
        self.backfilling
            .lock()
            .expect("backfill set")
            .insert((account_id, folder.to_owned()))
    }

    /// Release the backfill claim for `(account_id, folder)` once it finishes (or fails).
    pub fn end_backfill(&self, account_id: i64, folder: &str) {
        self.backfilling
            .lock()
            .expect("backfill set")
            .remove(&(account_id, folder.to_owned()));
    }

    /// Claim the IDLE watch for `account_id`. `Some(cancel)` if this caller should start the watcher
    /// (and the token to stop it on); `None` if one is already running. The watcher passes the same token
    /// to [`Self::release_idle_watch`] when it stops.
    pub fn claim_idle_watch(&self, account_id: i64) -> Option<Arc<tokio::sync::Notify>> {
        use std::collections::hash_map::Entry;
        match self
            .idle_watchers
            .lock()
            .expect("idle set")
            .entry(account_id)
        {
            Entry::Occupied(_) => None,
            Entry::Vacant(slot) => {
                let cancel = Arc::new(tokio::sync::Notify::new());
                slot.insert(cancel.clone());
                Some(cancel)
            }
        }
    }

    /// Free `account_id`'s slot when its watcher stops — but only if the slot still holds **this**
    /// watcher's `cancel` token. An account removed and re-added reuses its id (SQLite doesn't
    /// `AUTOINCREMENT`), so a successor watcher may already own the slot with a *different* token; the
    /// pointer check stops a departing watcher from evicting its replacement.
    pub fn release_idle_watch(&self, account_id: i64, cancel: &Arc<tokio::sync::Notify>) {
        let mut map = self.idle_watchers.lock().expect("idle set");
        if map.get(&account_id).is_some_and(|c| Arc::ptr_eq(c, cancel)) {
            map.remove(&account_id);
        }
    }

    /// Stop `account_id`'s IDLE watcher (on removal): fire its cancel so it drops its connection promptly,
    /// and free the slot so the next account to take this id can be watched. A no-op if it isn't watched.
    pub fn stop_idle_watch(&self, account_id: i64) {
        if let Some(cancel) = self
            .idle_watchers
            .lock()
            .expect("idle set")
            .remove(&account_id)
        {
            cancel.notify_one();
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
    pub fn wake_sync(&self) -> Arc<tokio::sync::Notify> {
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
    tokio::task::spawn_blocking(move || {
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

pub async fn list_accounts(state: &AppState) -> Result<Vec<AccountDto>, String> {
    with_store(state.clone(), |store| {
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

pub async fn list_folders(state: &AppState, account_id: i64) -> Result<Vec<FolderDto>, String> {
    with_store(state.clone(), move |store| {
        let mut folders = store
            .folders_for_account(account_id)
            .map_err(|_| "Couldn't read your folders.".to_owned())?;
        // The provider's Drafts folder is not a rail entry of its own: the **Drafts** entry *is* it
        // (its contents are merged into that list by `list_drafts`). Leaving it here would show
        // "Drafts" twice — once as the folder, once as the list of what's in it.
        // Every folder that IS the drafts folder — not just the one we resolved to. A server that
        // flags two (a locale migration that left both `Drafts` and `Entwürfe` marked) would otherwise
        // leave the loser in the rail, with the drafts icon, next to a Drafts entry that doesn't hold
        // its mail: the duplicate this was built to remove.
        let drafts = crate::dto::resolve_folder(&folders, crate::dto::FolderRole::Drafts);
        folders.retain(|f| f.role.as_deref() != Some("drafts") && Some(&f.name) != drafts.as_ref());
        folders.sort_by(|a, b| {
            folder_rank(&a.name, a.role.as_deref())
                .cmp(&folder_rank(&b.name, b.role.as_deref()))
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
                role: f.role,
            })
            .collect())
    })
    .await
}

/// Remove an account from this device (SEC-3): keychain password + local mail. Worker (keychain +
/// SQLite). Returns whether the keychain password was cleared cleanly.
pub async fn remove_account(state: &AppState, account_id: i64) -> Result<bool, String> {
    // Stop its IDLE watcher first, so it drops the authenticated connection promptly and frees the slot
    // — the account's id can be reused by the next account added, which must then be watchable.
    state.stop_idle_watch(account_id);
    let (db, secrets) = (state.db_path.clone(), state.secrets.clone());
    tokio::task::spawn_blocking(move || {
        geleit_engine::sync_actions::run_remove_account(&db, &*secrets, account_id)
    })
    .await
    .map_err(|_| "The task stopped unexpectedly.".to_owned())?
}

/// A boolean setting persisted in the store's `setting` k/v table (block-remote-images, mark-read,
/// notify). Read/written by the settings window; defaults handled on the frontend.
pub async fn get_bool_setting(state: &AppState, key: String) -> Result<Option<bool>, String> {
    with_store(state.clone(), move |store| {
        Ok(store
            .get_setting(&key)
            .map_err(|_| "Couldn't read your settings.".to_owned())?
            .map(|v| v == "1" || v == "true"))
    })
    .await
}

pub async fn set_bool_setting(state: &AppState, key: String, value: bool) -> Result<(), String> {
    with_store(state.clone(), move |store| {
        store
            .set_setting(&key, if value { "1" } else { "0" })
            .map_err(|_| "Couldn't save your setting.".to_owned())
    })
    .await
}

/// A free-text setting (quiet hours, so far). Same k/v table as [`get_bool_setting`]; `None` = unset,
/// and the frontend supplies the default.
pub async fn get_setting(state: &AppState, key: String) -> Result<Option<String>, String> {
    with_store(state.clone(), move |store| {
        store
            .get_setting(&key)
            .map_err(|_| "Couldn't read your settings.".to_owned())
    })
    .await
}

pub async fn set_setting(state: &AppState, key: String, value: String) -> Result<(), String> {
    with_store(state.clone(), move |store| {
        store
            .set_setting(&key, &value)
            .map_err(|_| "Couldn't save your setting.".to_owned())
    })
    .await
}

/// The account's signature (for the settings editor). `set_signature` persists it.
pub async fn get_signature(state: &AppState, account_id: i64) -> Result<String, String> {
    with_store(state.clone(), move |store| {
        Ok(store
            .signature(account_id)
            .map_err(|_| "Couldn't read your signature.".to_owned())?
            .unwrap_or_default())
    })
    .await
}

pub async fn set_signature(
    state: &AppState,
    account_id: i64,
    signature: String,
) -> Result<(), String> {
    with_store(state.clone(), move |store| {
        store
            .update_signature(account_id, &signature)
            .map_err(|_| "Couldn't save your signature.".to_owned())
    })
    .await
}

pub async fn list_messages(
    state: &AppState,
    folder_id: i64,
    limit: i64,
) -> Result<Vec<MessageDto>, String> {
    with_store(state.clone(), move |store| {
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
pub async fn list_all_messages(state: &AppState, limit: i64) -> Result<Vec<MessageDto>, String> {
    with_store(state.clone(), move |store| {
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

pub async fn open_message(
    state: &AppState,
    id: i64,
    mark_read: bool,
) -> Result<MessageBodyDto, String> {
    let st = state.clone();
    let (dto, account) = with_store(st.clone(), move |store| {
        let header = store
            .header_by_id(id)
            .map_err(|_| "Couldn't open this message.".to_owned())?
            .ok_or_else(|| "That message is no longer here.".to_owned())?;
        let body = store
            .body_for(id)
            .map_err(|_| "Couldn't read this message.".to_owned())?
            .unwrap_or_default();
        // Opening a message marks it read (READ-7) — persisted here, not just in the UI's signal, or
        // the unread dot reappears the moment the folder is re-listed from SQLite. A failure to record
        // it must not stop the user reading their mail, so it is best-effort. When the "mark as read
        // when opened" preference is off, the read is skipped entirely. `set_seen` marks the flag
        // dirty, so the SYNC-5 pull won't revert it before the write-back below confirms.
        let account = if mark_read {
            let _ = store.set_seen(id, true);
            store.account_for_message(id).ok().flatten()
        } else {
            None
        };
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
        Ok((
            MessageBodyDto {
                id: header.id,
                subject: display_subject(header.subject.as_deref()),
                from: display_sender(header.from_name.as_deref(), header.from_addr.as_deref()),
                date: header.date,
                plain: body.plain,
                is_html,
                has_remote,
                attachments,
            },
            account,
        ))
    })
    .await?;

    // Write the read back to the server (`\Seen`) so it shows as read on the user's other devices too,
    // via the durable queue — an immediate attempt now, retried every sweep until it lands. Reading
    // mail must never wait on the network (P1), so it's off the UI thread.
    if let Some(account_id) = account {
        spawn_flush(&st, account_id);
    }
    Ok(dto)
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
/// Drain this account's pending flag write-backs now, off the UI thread (SYNC-5). Fire-and-forget for
/// low latency: the change reaches the server within moments when online, and if this attempt fails the
/// row stays dirty and the scheduler retries it every sweep — so nothing is lost. `run_flush_flags`
/// clears the dirty marker for each message it confirms.
fn spawn_flush(state: &AppState, account_id: i64) {
    // Single-flight: if a flush for this account is already running, just ask it to run once more when
    // it's done (so a change made mid-flush still goes out) and return — no second thread, no herd.
    {
        let mut guard = state.flushing.lock().expect("flush guard");
        if let Some(rerun) = guard.get_mut(&account_id) {
            *rerun = true;
            return;
        }
        guard.insert(account_id, false);
    }
    let (db_path, secrets, flushing) = (
        state.db_path.clone(),
        state.secrets.clone(),
        state.flushing.clone(),
    );
    std::thread::spawn(move || loop {
        let _ = geleit_engine::sync_actions::run_flush_flags(&db_path, &*secrets, account_id);
        let mut guard = flushing.lock().expect("flush guard");
        match guard.get_mut(&account_id) {
            // Work arrived while we were flushing — clear the request and go round once more.
            Some(rerun) if *rerun => *rerun = false,
            // Nothing new; release the account so the next change can start a fresh flush.
            _ => {
                guard.remove(&account_id);
                break;
            }
        }
    });
}

/// Star / unstar a message (ORG-4). Optimistic local write + server write-back.
pub async fn set_star(state: &AppState, id: i64, on: bool) -> Result<(), String> {
    let st = state.clone();
    let account = with_store(st.clone(), move |store| {
        store
            .set_flagged(id, on)
            .map_err(|_| "Couldn't update the star.".to_owned())?;
        Ok(store.account_for_message(id).ok().flatten())
    })
    .await?;
    if let Some(account_id) = account {
        spawn_flush(&st, account_id);
    }
    Ok(())
}

/// Mark a message read (READ-7, for bulk mark-read). Optimistic local write + server write-back.
pub async fn set_read(state: &AppState, id: i64) -> Result<(), String> {
    set_seen_and_writeback(state, id, true, "Couldn't mark read.").await
}

/// Mark a message unread again (READ-7). Optimistic local write + server write-back.
pub async fn set_unread(state: &AppState, id: i64) -> Result<(), String> {
    set_seen_and_writeback(state, id, false, "Couldn't mark unread.").await
}

/// Shared body for `set_read`/`set_unread`: persist the seen flag locally, then write it back to the
/// server (`\Seen`) on a worker, targeting the message's real folder.
async fn set_seen_and_writeback(
    state: &AppState,
    id: i64,
    seen: bool,
    err: &'static str,
) -> Result<(), String> {
    let st = state.clone();
    let account = with_store(st.clone(), move |store| {
        store.set_seen(id, seen).map_err(|_| err.to_owned())?;
        Ok(store.account_for_message(id).ok().flatten())
    })
    .await?;
    if let Some(account_id) = account {
        spawn_flush(&st, account_id);
    }
    Ok(())
}

/// Move a message to a well-known folder by role — archive / trash / spam / un-spam (ORG-1/2/3).
/// Removes it from the current folder locally (optimistic) and moves it on the server. Returns
/// whether it acted (false = the account has no such folder, so nothing was done).
pub async fn move_to_role(state: &AppState, id: i64, role: String) -> Result<bool, String> {
    use crate::dto::{resolve_folder, FolderRole};
    let role = match role.as_str() {
        "archive" => FolderRole::Archive,
        "trash" => FolderRole::Trash,
        "spam" => FolderRole::Junk,
        "inbox" => FolderRole::Inbox,
        _ => return Err("Unknown action.".to_owned()),
    };
    let st = state.clone();

    // Plan the move, then record it as a `pending_move` (OFF-4). The row is not deleted here and not
    // moved on the server here: the marker hides it at once (the move feels instant, no network needed)
    // and is the durable queue the flush drains. A move made offline is safely recorded and pushed on
    // reconnect; the message stays in the store, hidden, until the server confirms — never lost.
    let plan = with_store(st.clone(), move |store| {
        let Some((source, _uid)) = store
            .message_location(id)
            .map_err(|_| "Couldn't move the message.".to_owned())?
        else {
            return Ok(None); // no server location (e.g. a local Saved message) — nothing to move
        };
        let account_id = store
            .account_for_message(id)
            .map_err(|_| "Couldn't move the message.".to_owned())?
            .ok_or_else(|| "Couldn't move the message.".to_owned())?;
        let folders = store
            .folders_for_account(account_id)
            .map_err(|_| "Couldn't move the message.".to_owned())?;
        let Some(target) = resolve_folder(&folders, role) else {
            return Ok(None); // account has no such folder — decline rather than invent one
        };
        if target == source {
            return Ok(None); // already there
        }
        Ok(Some((account_id, target)))
    })
    .await?;

    let Some((account_id, target)) = plan else {
        return Ok(false);
    };

    // Record the move locally — instant, offline-safe (OFF-4). The message vanishes from the list now;
    // the actual server move happens in the flush below (when online) or on the next reconnect.
    with_store(st.clone(), move |store| {
        store
            .queue_move(id, &target)
            .map_err(|_| "Couldn't move the message.".to_owned())
    })
    .await?;
    // Push it to the server now if we can reach it, so an online move settles at once. Offline this is a
    // no-op that leaves it queued for the scheduler to retry — either way the move is already recorded.
    flush_moves(&st, account_id).await;
    Ok(true)
}

/// Move a message to a folder **by name** — the folder the user picked from the Move… menu.
///
/// Distinct from [`move_to_role`], which is for the toolbar's Archive / Delete / Spam and has to *find*
/// the folder. Here the user has already named it, and any guessing on our part would be a bug: the
/// menu used to map every folder it listed onto one of four roles, so moving a message into an ordinary
/// folder filed it in the **Inbox** instead.
///
/// Returns whether it acted (false = no such folder, or it's already there).
pub async fn move_to_folder(state: &AppState, id: i64, folder: String) -> Result<bool, String> {
    let st = state.clone();
    let target = folder.clone();
    let plan = with_store(st.clone(), move |store| {
        let Some((source, _uid)) = store
            .message_location(id)
            .map_err(|_| "Couldn't move the message.".to_owned())?
        else {
            return Ok(None); // no server location (e.g. a local Saved message)
        };
        let account_id = store
            .account_for_message(id)
            .map_err(|_| "Couldn't move the message.".to_owned())?
            .ok_or_else(|| "Couldn't move the message.".to_owned())?;
        // The folder must be one this account actually has — never take a name on trust and create a
        // mailbox out of a typo.
        let known = store
            .folders_for_account(account_id)
            .map_err(|_| "Couldn't read your folders.".to_owned())?
            .into_iter()
            .any(|f| f.name == target);
        if !known || target == source {
            return Ok(None);
        }
        Ok(Some(account_id))
    })
    .await?;
    let Some(account_id) = plan else {
        return Ok(false);
    };

    // Queue the move (OFF-4) — instant and offline-safe, exactly as in `move_to_role`. The message
    // hides now; the flush below pushes it to the server when online, or the scheduler does on reconnect.
    with_store(st.clone(), move |store| {
        store
            .queue_move(id, &folder)
            .map_err(|_| "Couldn't move the message.".to_owned())
    })
    .await?;
    flush_moves(&st, account_id).await;
    Ok(true)
}

/// Empty the account's Trash (ORG-2): permanently delete everything in it, on the server and locally.
/// Irreversible — the UI confirms first.
pub async fn empty_trash(state: &AppState, account_id: i64) -> Result<(), String> {
    use crate::dto::{resolve_folder, FolderRole};
    let st = state.clone();
    // Resolve the Trash folder (name for the server call, id for the local clear).
    let trash = with_store(st.clone(), move |store| {
        let folders = store
            .folders_for_account(account_id)
            .map_err(|_| "Couldn't read your folders.".to_owned())?;
        Ok(
            resolve_folder(&folders, FolderRole::Trash).and_then(|name| {
                folders
                    .iter()
                    .find(|f| f.name == name)
                    .map(|f| (name.clone(), f.id))
            }),
        )
    })
    .await?;
    let Some((name, folder_id)) = trash else {
        return Err("This account has no Trash folder.".to_owned());
    };
    // Empty on the server first (blocking); only then clear the local rows, so a failure keeps them.
    let (db, secrets) = (st.db_path.clone(), st.secrets.clone());
    tokio::task::spawn_blocking(move || {
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
pub async fn delete_forever(state: &AppState, id: i64) -> Result<(), String> {
    let st = state.clone();
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
        tokio::task::spawn_blocking(move || {
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
pub async fn create_folder(state: &AppState, account_id: i64, name: String) -> Result<i64, String> {
    let name = validate_folder_name(&name)?;
    let st = state.clone();
    let (db, secrets) = (st.db_path.clone(), st.secrets.clone());
    let name2 = name.clone();
    tokio::task::spawn_blocking(move || {
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
pub async fn rename_folder(
    state: &AppState,
    account_id: i64,
    from: String,
    to: String,
) -> Result<(), String> {
    let st = state.clone();
    // A special folder is special whatever it is *called* — so ask the store for its role, not just its
    // name. Renaming the folder the server marked `\Trash` breaks the account, and on a provider that
    // calls it `Papierkorb` the name list alone would have let it through.
    let role = folder_role(&st, account_id, &from).await;
    if is_protected_folder(&from, role.as_deref()) {
        return Err("That folder can't be renamed.".to_owned());
    }
    let to = validate_folder_name(&to)?;
    // Don't let a user rename an ordinary folder *into* a reserved name — that would mint a
    // role-named folder the UI then treats as protected (un-renamable, un-deletable).
    if is_protected_folder(&to, None) {
        return Err("That name is reserved for a standard folder.".to_owned());
    }
    let (db, secrets) = (st.db_path.clone(), st.secrets.clone());
    let (from2, to2) = (from.clone(), to.clone());
    tokio::task::spawn_blocking(move || {
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
pub async fn delete_folder(
    state: &AppState,
    account_id: i64,
    folder_id: i64,
    name: String,
) -> Result<(), String> {
    let st = state.clone();
    let role = folder_role(&st, account_id, &name).await;
    if is_protected_folder(&name, role.as_deref()) {
        return Err("That folder can't be deleted.".to_owned());
    }
    let (db, secrets) = (st.db_path.clone(), st.secrets.clone());
    let name2 = name.clone();
    tokio::task::spawn_blocking(move || {
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

/// Add (or reconnect) an account: validate the form, create the account, store the password in the
/// keychain, and do a first sync (S9.6). Worker — network + keychain (P1). Returns the account id.
#[allow(clippy::too_many_arguments)]
pub async fn add_account(
    state: &AppState,
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
    let account_id = tokio::task::spawn_blocking(move || {
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
    .map_err(|_| "The setup task stopped unexpectedly.".to_owned())??;
    // Returning the id lets the host wire up host-specific side-effects for the new account — the
    // Tauri shell starts an instant-IDLE watcher for it; the background poll covers it regardless.
    Ok(account_id)
}

/// Search an account's mail (FTS5, M6). Instant + local (P1); supports `from:`/`subject:`/
/// `has:attachment` operators. Returns headers as list rows.
pub async fn search(
    state: &AppState,
    account_id: i64,
    query: String,
) -> Result<Vec<MessageDto>, String> {
    with_store(state.clone(), move |store| {
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
pub async fn search_all(state: &AppState, query: String) -> Result<Vec<MessageDto>, String> {
    with_store(state.clone(), move |store| {
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
pub async fn set_theme(state: &AppState, theme: String) -> Result<(), String> {
    with_store(state.clone(), move |store| {
        store
            .set_setting("theme", &theme)
            .map_err(|_| "Couldn't save your setting.".to_owned())
    })
    .await
}

/// Build a reply / reply-all / forward draft, prefilled from a stored message (S9.5). Pure over the
/// store — no network. `kind` is "reply" | "reply_all" | "forward".
pub async fn compose_draft(
    state: &AppState,
    id: i64,
    kind: String,
) -> Result<crate::dto::ComposeDraft, String> {
    with_store(state.clone(), move |store| {
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
#[allow(clippy::too_many_arguments)]
pub async fn send_message(
    state: &AppState,
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
    outbox_edit_id: Option<i64>,
) -> Result<bool, String> {
    // Append the account's signature (SEND-7). Read it up front on the store thread.
    let signature = with_store(state.clone(), move |store| {
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
    let status = tokio::task::spawn_blocking(move || {
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
            outbox_edit_id, // …and if it was an edit of a rejected send, drops that outbox row too
        )
    })
    .await
    .map_err(|_| "The send task stopped unexpectedly.".to_owned())??;
    // `true` = queued in the outbox (offline), so the UI can say so instead of "Sent".
    Ok(status == geleit_engine::sync_actions::SendStatus::Queued)
}

/// How many messages are waiting to send, and how many the server rejected — for the outbox indicator
/// (SEND-10). `(queued, failed)`.
pub async fn outbox_status(state: &AppState) -> Result<(i64, i64), String> {
    with_store(state.clone(), |store| {
        store
            .outbox_counts()
            .map_err(|_| "Couldn't read the outbox.".to_owned())
    })
    .await
}

/// Every message in the outbox, for the outbox view (SEND-10) — so the user can retry or discard one.
pub async fn list_outbox(state: &AppState) -> Result<Vec<crate::dto::OutboxItemDto>, String> {
    with_store(state.clone(), |store| {
        Ok(store
            .list_outbox()
            .map_err(|_| "Couldn't read the outbox.".to_owned())?
            .into_iter()
            .map(crate::dto::outbox_item)
            .collect())
    })
    .await
}

/// Re-queue a failed outbox message (SEND-10) and try it now: clear its failed mark, then flush the
/// outbox so it goes out immediately if we're online (rather than waiting for the next sweep).
pub async fn retry_outbox(state: &AppState, id: i64) -> Result<(), String> {
    let st = state.clone();
    let account = with_store(st.clone(), move |store| {
        store
            .retry_outbox(id)
            .map_err(|_| "Couldn't retry that message.".to_owned())?;
        Ok(store.outbox_account(id).ok().flatten())
    })
    .await?;
    if let Some(account_id) = account {
        flush_outbox(&st, account_id).await;
    }
    Ok(())
}

/// Discard an outbox message (SEND-10) — throw away a send that's waiting or couldn't go out.
pub async fn discard_outbox(state: &AppState, id: i64) -> Result<(), String> {
    with_store(state.clone(), move |store| {
        store
            .delete_outbox(id)
            .map_err(|_| "Couldn't discard that message.".to_owned())
    })
    .await
}

/// Reopen a queued message in the composer to edit it (SEND-10) — e.g. fix the address a send was
/// rejected for, instead of discarding and retyping. Parses the stored raw bytes back into a compose
/// form and materialises its attachments to temp files, exactly like resuming a draft. `None` if the
/// row is gone (already sent or discarded).
///
/// The outbox row is **left in place** — it's removed only when the edited message is sent (the
/// frontend passes its id back to `discard_outbox` on send). So cancelling the compose loses nothing:
/// the original stays in the outbox to retry or discard. Only offered on **failed** rows, which the
/// scheduler never retries, so there's no risk of the original going out while it's being edited.
pub async fn edit_outbox(state: &AppState, id: i64) -> Result<Option<ResumedDraft>, String> {
    with_store(state.clone(), move |store| {
        let Some(raw) = store
            .outbox_raw(id)
            .map_err(|_| "Couldn't open that message.".to_owned())?
        else {
            return Ok(None);
        };
        let edit = geleit_engine::message::parse_outbox_for_edit(&raw);
        let base = std::env::temp_dir().join(format!("geleit-outbox-{id}"));
        let attachments = materialize_attachments(
            &base,
            edit.attachments.iter().map(|(f, d)| (f.as_deref(), &d[..])),
        );
        Ok(Some(ResumedDraft {
            draft: ComposeDraft {
                to: edit.to,
                cc: edit.cc,
                subject: edit.subject,
                body: edit.body,
                in_reply_to: None,
                references: Vec::new(),
            },
            attachments,
        }))
    })
    .await
}

// --- Snooze (ORG-9) -------------------------------------------------------------------------------

/// The snooze times to offer, computed in the user's local timezone (ORG-9). Only future ones.
pub async fn snooze_presets() -> Result<Vec<crate::dto::SnoozePresetDto>, String> {
    Ok(crate::snooze::presets(chrono::Local::now())
        .into_iter()
        .map(|p| crate::dto::SnoozePresetDto {
            label: p.label,
            at: p.at,
        })
        .collect())
}

/// Snooze messages until `until` (a unix timestamp): hide them until then. Refreshes the badge, since a
/// snoozed unread message stops counting.
pub async fn snooze_messages(
    shell: &dyn Shell,
    state: &AppState,
    ids: Vec<i64>,
    until: i64,
) -> Result<(), String> {
    let st = state.clone();
    with_store(st.clone(), move |store| {
        store
            .snooze_messages(&ids, until)
            .map_err(|_| "Couldn't snooze that message.".to_owned())
    })
    .await?;
    set_badge(shell, &st).await;
    Ok(())
}

/// Bring a snoozed message back now (ORG-9). Refreshes the badge (it may count again).
pub async fn unsnooze_message(shell: &dyn Shell, state: &AppState, id: i64) -> Result<(), String> {
    let st = state.clone();
    with_store(st.clone(), move |store| {
        store
            .unsnooze_message(id)
            .map_err(|_| "Couldn't un-snooze that message.".to_owned())
    })
    .await?;
    set_badge(shell, &st).await;
    Ok(())
}

/// The messages still snoozed for an account, soonest-first — for the Snoozed view (ORG-9). Each row's
/// resurface time is phrased in the user's local timezone here, so the UI just shows it.
pub async fn list_snoozed(
    state: &AppState,
    account_id: i64,
) -> Result<Vec<crate::dto::SnoozedItemDto>, String> {
    with_store(state.clone(), move |store| {
        Ok(store
            .snoozed_messages(account_id)
            .map_err(|_| "Couldn't read your snoozed mail.".to_owned())?
            .into_iter()
            .map(|s| {
                let when = format_local_when(s.snoozed_until);
                crate::dto::snoozed_item(s, when)
            })
            .collect())
    })
    .await
}

/// Phrase a unix timestamp as a short local-time label for the Snoozed view, e.g. "Tue 21 Jul, 08:00".
/// Falls back to the raw number only if the timestamp is out of range (never, in practice).
fn format_local_when(ts: i64) -> String {
    use chrono::TimeZone;
    match chrono::Local.timestamp_opt(ts, 0).single() {
        Some(dt) => dt.format("%a %-d %b, %H:%M").to_string(),
        None => ts.to_string(),
    }
}

/// Resurface any snoozed mail whose time has come (ORG-9), across every account. Returns how many, so
/// the scheduler knows the list + badge are stale. Local and connectivity-independent.
pub async fn resurface_snoozes(state: &AppState) -> usize {
    with_store(state.clone(), |store| {
        store.resurface_due_snoozes().map_err(|_| String::new())
    })
    .await
    .unwrap_or(0)
}

// --- Rules / filters (ORG-8) ----------------------------------------------------------------------

/// Current unix time in seconds (for a rule's `created_at`). Saturates to 0 before the epoch.
fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// What happened to a matched message's move.
enum MoveOutcome {
    /// Moved on the server and dropped locally — it left the inbox.
    Moved,
    /// The move can't apply (target folder gone, or no server location): treat as done, don't retry.
    Unappliable,
    /// A network failure: leave `filtered = 0` so the next sweep retries.
    Retry,
}

/// Apply an account's rules to its INBOX mail awaiting a pass (ORG-8). First-match-wins per message;
/// flag actions are local writes + the SYNC-5 write-back queue, a move is server-first then the local
/// row is dropped. A message is marked `filtered` only once its actions land — a move that fails offline
/// stays unfiltered so the next sweep retries (re-applying idempotent flag actions is harmless). Returns
/// how many messages a rule acted on. Runs on the worker path (the move is network); called by the
/// scheduler after each INBOX sync and by **Run on inbox now**.
pub async fn apply_rules(state: &AppState, account_id: i64) -> usize {
    // Compute the match plan under one store read: (message id, the rule it matched, if any).
    let plan: Vec<(i64, Option<geleit_store::Rule>)> = with_store(state.clone(), move |store| {
        let rules = store.list_rules(account_id).unwrap_or_default();
        let msgs = store.unfiltered_inbox(account_id).unwrap_or_default();
        Ok(msgs
            .into_iter()
            .map(|m| {
                let matched = rules
                    .iter()
                    .find(|r| {
                        geleit_core::rule::RuleField::from_key(&r.field).is_some_and(|f| {
                            geleit_core::rule::matches(
                                f,
                                &r.pattern,
                                m.from_name.as_deref(),
                                m.from_addr.as_deref(),
                                m.subject.as_deref(),
                                m.to_addrs.as_deref(),
                            )
                        })
                    })
                    .cloned();
                (m.id, matched)
            })
            .collect())
    })
    .await
    .unwrap_or_default();

    // Pass 1 — apply every matched flag action locally (mark-read / star), across all messages.
    let mut flagged = false;
    for (id, matched) in &plan {
        if let Some(rule) = matched {
            if rule.mark_read || rule.star {
                let (id, mark_read, star) = (*id, rule.mark_read, rule.star);
                let _ = with_store(state.clone(), move |store| {
                    if mark_read {
                        let _ = store.set_seen(id, true);
                    }
                    if star {
                        let _ = store.set_flagged(id, true);
                    }
                    Ok(())
                })
                .await;
                flagged = true;
            }
        }
    }

    // Push those flags to the server **now**, before any move below — an IMAP MOVE carries the
    // message's current flags, so `\Seen`/`\Flagged` must be on the server copy *first* or a
    // "move + mark read" rule would file the message still unread (and deleting the moved row would
    // drop the deferred write-back with it). Synchronous + best-effort: offline it fails and the
    // ordinary SYNC-5 write-back queue pushes the flags on a later sweep instead.
    if flagged {
        let (db, secrets) = (state.db_path.clone(), state.secrets.clone());
        let _ = tokio::task::spawn_blocking(move || {
            geleit_engine::sync_actions::run_flush_flags(&db, &*secrets, account_id)
        })
        .await;
    }

    // Pass 2 — moves (now the server copy carries the right flags), then mark the rest done.
    let mut acted = 0usize;
    for (id, matched) in plan {
        let Some(rule) = matched else {
            // No rule matched — it's been evaluated; don't look at it again.
            let _ = with_store(state.clone(), move |store| {
                store.mark_filtered(id).map_err(|_| String::new())
            })
            .await;
            continue;
        };
        let flagged_this = rule.mark_read || rule.star;

        if let Some(folder) = rule.target_folder.clone() {
            match rule_move(state, account_id, id, folder).await {
                MoveOutcome::Moved => {
                    // Mark filtered *and* drop the local row: if the delete somehow fails, the
                    // `filtered = 1` still stops the message looping (the next real sync reconciles the
                    // now-moved UID away), rather than retrying a move against a UID that's gone.
                    let _ = with_store(state.clone(), move |store| {
                        let _ = store.mark_filtered(id);
                        store.delete_message(id).map_err(|_| String::new())
                    })
                    .await;
                    acted += 1;
                }
                // Can't apply the move (folder gone / already there) — flags may have applied; mark it
                // done so it doesn't loop forever.
                MoveOutcome::Unappliable => {
                    let _ = with_store(state.clone(), move |store| {
                        store.mark_filtered(id).map_err(|_| String::new())
                    })
                    .await;
                    acted += 1;
                }
                // Network failure — leave it unfiltered; the next sweep retries the move. The flags
                // already applied, so still count it as a visible change (the list must re-list).
                MoveOutcome::Retry => {
                    if flagged_this {
                        acted += 1;
                    }
                }
            }
            continue;
        }

        // A flag-only rule: flags are applied + pushed above; mark it done.
        let _ = with_store(state.clone(), move |store| {
            store.mark_filtered(id).map_err(|_| String::new())
        })
        .await;
        acted += 1;
    }
    acted
}

/// Move one matched message into `folder` on the server (ORG-8), the same server-first path as
/// `move_to_folder`. The caller drops the local row on [`MoveOutcome::Moved`].
async fn rule_move(state: &AppState, account_id: i64, id: i64, folder: String) -> MoveOutcome {
    let target = folder.clone();
    // Resolve the server location and confirm the target is a real folder of this account.
    let plan = with_store(state.clone(), move |store| {
        let Some((source, uid)) = store.message_location(id).ok().flatten() else {
            return Ok(None);
        };
        let known = store
            .folders_for_account(account_id)
            .map(|fs| fs.into_iter().any(|f| f.name == target))
            .unwrap_or(false);
        if !known || target == source {
            return Ok(None);
        }
        Ok(Some((source, uid)))
    })
    .await
    .unwrap_or(None);
    let Some((source, uid)) = plan else {
        return MoveOutcome::Unappliable; // folder gone, or already there / no server copy
    };
    let (db, secrets) = (state.db_path.clone(), state.secrets.clone());
    let moved = tokio::task::spawn_blocking(move || {
        geleit_engine::sync_actions::run_move(
            &db, &*secrets, account_id, &source, uid as u32, &folder,
        )
    })
    .await;
    match moved {
        Ok(Ok(())) => MoveOutcome::Moved,
        _ => MoveOutcome::Retry, // network/server failure — retry next sweep
    }
}

/// An account's rules, in evaluation order (ORG-8).
pub async fn list_rules(
    state: &AppState,
    account_id: i64,
) -> Result<Vec<crate::dto::RuleDto>, String> {
    with_store(state.clone(), move |store| {
        Ok(store
            .list_rules(account_id)
            .map_err(|_| "Couldn't read your rules.".to_owned())?
            .into_iter()
            .map(crate::dto::rule_dto)
            .collect())
    })
    .await
}

/// Add a rule (ORG-8). Validates the field, a non-empty pattern, and at least one action.
#[allow(clippy::too_many_arguments)]
pub async fn add_rule(
    state: &AppState,
    account_id: i64,
    field: String,
    pattern: String,
    target_folder: Option<String>,
    mark_read: bool,
    star: bool,
) -> Result<i64, String> {
    if geleit_core::rule::RuleField::from_key(&field).is_none() {
        return Err("Pick what the rule looks at.".to_owned());
    }
    if pattern.trim().is_empty() {
        return Err("Type the text the rule should match.".to_owned());
    }
    let folder = target_folder.filter(|f| !f.trim().is_empty());
    if folder.is_none() && !mark_read && !star {
        return Err("Choose at least one thing for the rule to do.".to_owned());
    }
    with_store(state.clone(), move |store| {
        store
            .add_rule(
                account_id,
                &field,
                pattern.trim(),
                folder.as_deref(),
                mark_read,
                star,
                now_secs(),
            )
            .map_err(|_| "Couldn't save the rule.".to_owned())
    })
    .await
}

/// Delete a rule (ORG-8).
pub async fn delete_rule(state: &AppState, id: i64) -> Result<(), String> {
    with_store(state.clone(), move |store| {
        store
            .delete_rule(id)
            .map_err(|_| "Couldn't delete that rule.".to_owned())
    })
    .await
}

/// Move a rule up or down its account's evaluation order (ORG-8) — rules are first-match-wins, so this
/// sets priority. A no-op at the edges.
pub async fn move_rule(state: &AppState, id: i64, up: bool) -> Result<(), String> {
    with_store(state.clone(), move |store| {
        store
            .move_rule(id, up)
            .map_err(|_| "Couldn't reorder that rule.".to_owned())
    })
    .await
}

/// **Run on inbox now** (ORG-8): re-arm the whole INBOX for a rule pass and apply the rules to it, so a
/// rule the user just added tidies the mail already sitting there. Returns how many messages it acted on.
pub async fn run_rules_now(
    shell: &dyn Shell,
    state: &AppState,
    account_id: i64,
) -> Result<i64, String> {
    let st = state.clone();
    with_store(st.clone(), move |store| {
        store
            .reset_inbox_filtered(account_id)
            .map(|_| ())
            .map_err(|_| "Couldn't run your rules.".to_owned())
    })
    .await?;
    let acted = apply_rules(&st, account_id).await;
    // A rule may have marked mail read or moved it out of the inbox — the badge could have moved.
    set_badge(shell, &st).await;
    Ok(acted as i64)
}

// --- Auto-update (APP-7) --------------------------------------------------------------------------
//
// Only the running version lives here — it's a plain constant. The *check* and *install* are
// inherently Tauri (`tauri-plugin-updater`), so they stay in the desktop host; the web host serves
// its own stub, since a self-hosted server is updated by its operator, not from inside a browser tab.

/// The running app version, for the Settings "Updates" block.
pub fn app_version() -> String {
    env!("CARGO_PKG_VERSION").to_owned()
}

/// Drain an account's outbox — a worker-awaited version for the scheduler and Refresh (SEND-10).
///
/// Single-flight per account: if a drain is already running (a sweep and a Refresh overlapping), this
/// one skips rather than deliver the same messages a second time — the running drain sends them all.
pub async fn flush_outbox(state: &AppState, account_id: i64) -> usize {
    {
        let mut g = state.draining_outbox.lock().expect("outbox guard");
        if let Some(rerun) = g.get_mut(&account_id) {
            *rerun = true; // a drain is running — have it go round once more, then skip our own
            return 0;
        }
        g.insert(account_id, false);
    }
    let mut sent = 0usize;
    loop {
        let (db, secrets) = (state.db_path.clone(), state.secrets.clone());
        sent += tokio::task::spawn_blocking(move || {
            geleit_engine::sync_actions::run_flush_outbox(&db, &*secrets, account_id).unwrap_or(0)
        })
        .await
        .unwrap_or(0);
        let mut g = state.draining_outbox.lock().expect("outbox guard");
        match g.get_mut(&account_id) {
            // Work arrived while we were draining (a Retry, say) — go round once more.
            Some(rerun) if *rerun => *rerun = false,
            _ => {
                g.remove(&account_id);
                break;
            }
        }
    }
    sent
}

/// Push this account's queued offline moves to the server (OFF-4). Single-flight per account, same as
/// [`flush_outbox`]: a sweep and a fresh move mustn't run the same `pending_move` rows at once. Returns
/// how many moves reached the server, so a caller can tell the on-screen list is now stale. Awaited (a
/// worker thread does the blocking IMAP work) so a move made online completes before the command
/// returns and the list settles immediately; offline it simply queues and the scheduler retries.
pub async fn flush_moves(state: &AppState, account_id: i64) -> usize {
    {
        let mut g = state.draining_moves.lock().expect("moves guard");
        if let Some(rerun) = g.get_mut(&account_id) {
            *rerun = true; // a flush is running — have it go round once more, then skip our own
            return 0;
        }
        g.insert(account_id, false);
    }
    let mut moved = 0usize;
    loop {
        let (db, secrets) = (state.db_path.clone(), state.secrets.clone());
        moved += tokio::task::spawn_blocking(move || {
            geleit_engine::sync_actions::run_flush_moves(&db, &*secrets, account_id).unwrap_or(0)
        })
        .await
        .unwrap_or(0);
        let mut g = state.draining_moves.lock().expect("moves guard");
        match g.get_mut(&account_id) {
            // A move was queued while we were flushing — go round once more so it goes out now too.
            Some(rerun) if *rerun => *rerun = false,
            _ => {
                g.remove(&account_id);
                break;
            }
        }
    }
    moved
}

/// Save (or update) a local draft (SEND-5). Returns the draft's id so the composer can keep editing
/// the same row. The local copy is encrypted at rest and is always the source of truth. When the
/// opt-in **"Sync drafts to server"** setting is on (default off, SEND-5), a copy is also appended to
/// the account's Drafts folder so other mail clients see it — best-effort: a server failure never
/// fails the local save.
pub async fn save_draft(
    state: &AppState,
    account_id: i64,
    draft_id: Option<i64>,
    draft: ComposeDraft,
    attachments: Vec<String>,
) -> Result<i64, String> {
    let st = state.clone();
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
        let synced = tokio::task::spawn_blocking(move || {
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

/// The role the server gave one of this account's folders (`None` = it said nothing, or the folder is
/// gone). Used to protect a special folder from being renamed or deleted whatever it is called.
async fn folder_role(state: &AppState, account_id: i64, name: &str) -> Option<String> {
    let name = name.to_owned();
    with_store(state.clone(), move |store| {
        Ok(store
            .folders_for_account(account_id)
            .ok()
            .and_then(|fs| fs.into_iter().find(|f| f.name == name))
            .and_then(|f| f.role))
    })
    .await
    .ok()
    .flatten()
}

/// The account's Drafts folder. `None` → the provider keeps none, so drafts live on this device (and
/// nothing is hidden from the rail).
///
/// The **server's** `\Drafts` flag decides, so a provider that calls it `Entwürfe` works; the English
/// name is only the fallback. Same answer everywhere — see [`geleit_core::pick_folder`].
fn drafts_folder(store: &Store, account_id: i64) -> Option<String> {
    let folders = store.folders_for_account(account_id).ok()?;
    crate::dto::resolve_folder(&folders, crate::dto::FolderRole::Drafts)
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
pub async fn list_drafts(state: &AppState, account_id: i64) -> Result<Vec<DraftSummary>, String> {
    with_store(state.clone(), move |store| {
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
pub async fn refresh_drafts(state: &AppState, account_id: i64) -> Result<bool, String> {
    let st = state.clone();
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
pub async fn resume_server_draft(state: &AppState, id: i64) -> Result<ResumedDraft, String> {
    let st = state.clone();
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
            let (fetched_name, bytes) = tokio::task::spawn_blocking(move || {
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
pub async fn load_draft(state: &AppState, id: i64) -> Result<Option<ResumedDraft>, String> {
    with_store(state.clone(), move |store| {
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
    materialize_attachments(
        &base,
        atts.iter().map(|a| (a.filename.as_deref(), &a.data[..])),
    )
}

/// Write attachment bytes to a per-message temp dir, one numbered sub-dir per file so same-named
/// files stay distinct while the **basename stays clean** (what the composer chip shows). The name is
/// sanitised so a hostile stored filename can't escape the temp dir. Best-effort — a file that can't
/// be written is skipped rather than failing the whole reopen. Returns the paths written.
fn materialize_attachments<'a>(
    base: &std::path::Path,
    atts: impl Iterator<Item = (Option<&'a str>, &'a [u8])>,
) -> Vec<String> {
    let mut paths = Vec::new();
    for (i, (filename, data)) in atts.enumerate() {
        let name = filename
            .map(safe_attachment_filename)
            .unwrap_or_else(|| format!("attachment-{}", i + 1));
        let dir = base.join(i.to_string());
        if std::fs::create_dir_all(&dir).is_err() {
            continue;
        }
        let path = dir.join(&name);
        if std::fs::write(&path, data).is_ok() {
            paths.push(path.to_string_lossy().into_owned());
        }
    }
    paths
}

/// Delete a saved draft (idempotent). Used by the draft-list delete affordance.
pub async fn delete_draft(state: &AppState, id: i64) -> Result<(), String> {
    let st = state.clone();
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
        let _ = tokio::task::spawn_blocking(move || {
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
pub async fn purge_server_drafts(state: &AppState, account_id: i64) -> Result<(), String> {
    let st = state.clone();
    let copies = with_store(st.clone(), move |store| {
        store
            .drafts_with_server_copies(account_id)
            .map_err(|_| "Couldn't read your drafts.".to_owned())
    })
    .await?;
    for (draft_id, folder, mid) in copies {
        let (db, secrets) = (st.db_path.clone(), st.secrets.clone());
        let gone = tokio::task::spawn_blocking(move || {
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
pub async fn suggest_addresses(
    state: &AppState,
    account_id: i64,
    prefix: String,
) -> Result<Vec<String>, String> {
    with_store(state.clone(), move |store| {
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
pub async fn pick_files() -> Result<Vec<String>, String> {
    tokio::task::spawn_blocking(|| {
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
/// Native "choose a folder" dialog (for the whole-account export destination). `None` if cancelled.
fn pick_directory() -> Result<Option<String>, String> {
    let attempts: [(&str, Vec<String>); 2] = [
        (
            "zenity",
            vec![
                "--file-selection".into(),
                "--directory".into(),
                "--title=Choose a folder to export into".into(),
            ],
        ),
        ("kdialog", vec!["--getexistingdirectory".into(), ".".into()]),
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
    Err("No folder picker found — install zenity or kdialog.".to_owned())
}

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

/// The `.eml` bytes + a suggested filename for message `id` (READ-10) — rebuilt from what's stored, no
/// network. The reusable core shared by the desktop's save dialog ([`save_eml`]) and the web host's
/// download route.
pub async fn eml_bytes(state: &AppState, id: i64) -> Result<(Vec<u8>, String), String> {
    with_store(state.clone(), move |store| {
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
    .await
}

/// Save an open message to disk as a `.eml` file (READ-10). Rebuilds RFC 822 bytes from what's stored
/// (no network), asks where to save via a native dialog, and writes. Returns whether a file was
/// written (`false` = the user cancelled the dialog).
pub async fn save_eml(state: &AppState, id: i64) -> Result<bool, String> {
    let (bytes, default_name) = eml_bytes(state, id).await?;
    tokio::task::spawn_blocking(move || {
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

/// Export a folder to an mbox file (SEC-4): pull each message's raw original from the server (best-effort;
/// offline falls back to the stored envelope + body), frame it mbox-style, and write the lot to a path
/// from the native save dialog. Returns an [`ExportSummary`] (how many written, and how many were
/// text-only), or `None` if the user cancels. (One folder's mbox is held in memory here — a deliberate
/// one-off; per-message streaming is a follow-up.)
pub async fn export_folder(
    state: &AppState,
    folder_id: i64,
    folder_name: String,
) -> Result<Option<crate::dto::ExportSummary>, String> {
    let st = state.clone();
    // Empty folder → say so, no dialog. Checked first (like `export_account`) so an empty folder never
    // opens a save dialog.
    let count = with_store(st.clone(), move |store| {
        store
            .folder_message_count(folder_id)
            .map_err(|_| "Couldn't read the folder to export.".to_owned())
    })
    .await?;
    if count == 0 {
        return Ok(Some(crate::dto::ExportSummary::default()));
    }
    // Choose the destination first, so the dialog appears immediately and a cancel costs no fetch/build.
    let default_name = format!("{}.mbox", safe_filename_stem(&folder_name));
    let Some(path) = tokio::task::spawn_blocking(move || pick_save_path(&default_name))
        .await
        .map_err(|_| "The export task stopped unexpectedly.".to_owned())??
    else {
        return Ok(None); // cancelled
    };
    let (mbox, written, text_only, _reachable) =
        folder_mbox_complete(st, folder_id, folder_name, true).await?;
    tokio::task::spawn_blocking(move || std::fs::write(&path, &mbox))
        .await
        .map_err(|_| "The export task stopped unexpectedly.".to_owned())?
        .map_err(|_| "Couldn't write that file.".to_owned())?;
    Ok(Some(crate::dto::ExportSummary {
        exported: written,
        text_only,
    }))
}

/// Export a whole account to a folder the user picks (SEC-4) — one `.mbox` file per mail folder, so the
/// folder structure is preserved (unlike jamming everything into a single file). Returns an
/// [`ExportSummary`] (messages written across all folders, and how many were text-only), an all-zero
/// summary if the account has none, or `None` if cancelled. Each message's raw original (attachments and
/// all) is pulled from the server when reachable; offline it falls back to the stored envelope + body.
///
/// **Streamed to disk:** the directory is chosen first, then each folder is built and written before the
/// next is started — so peak memory is one folder's mbox, never every folder's at once.
pub async fn export_account(
    state: &AppState,
    account_id: i64,
) -> Result<Option<crate::dto::ExportSummary>, String> {
    let st = state.clone();
    // The non-empty folders (id for the build, name for the filename + IMAP fetch). Empty folders are
    // dropped here so no empty `.mbox` is written and an empty account is known *before* the dialog.
    let folders = with_store(st.clone(), move |store| {
        let mut out: Vec<(i64, String)> = Vec::new();
        for f in store
            .folders_for_account(account_id)
            .map_err(|_| "Couldn't read your folders.".to_owned())?
        {
            if store
                .folder_message_count(f.id)
                .map_err(|_| "Couldn't read your folders.".to_owned())?
                > 0
            {
                out.push((f.id, f.name));
            }
        }
        Ok(out)
    })
    .await?;
    if folders.is_empty() {
        return Ok(Some(crate::dto::ExportSummary::default()));
    }
    // Choose the destination up front, so the dialog appears immediately (not after a long fetch) and we
    // can write each folder's file as it's built instead of holding them all.
    let Some(dir) = tokio::task::spawn_blocking(pick_directory)
        .await
        .map_err(|_| "The export task stopped unexpectedly.".to_owned())??
    else {
        return Ok(None); // cancelled
    };

    // Build and write folder by folder. `online` fails fast: the moment one folder's fetch finds the
    // server unreachable, the rest skip the network and degrade to reconstruction straight away.
    let mut exported = 0i64;
    let mut text_only = 0i64;
    let mut online = true;
    let mut wrote_any_folder = false;
    for (fid, fname) in folders {
        let built = folder_mbox_complete(st.clone(), fid, fname.clone(), online).await;
        let (bytes, written, folder_text_only, reachable) = match built {
            Ok(v) => v,
            // A store hiccup on one folder skips it, rather than losing the whole account's export.
            Err(_) => continue,
        };
        if !reachable {
            online = false; // stop probing every remaining folder
        }
        if written == 0 {
            continue;
        }
        let path = std::path::Path::new(&dir).join(format!("{}.mbox", safe_filename_stem(&fname)));
        // One folder failing to write skips just that folder — same best-effort stance as a build error
        // above, so a single bad filename or a transient I/O glitch doesn't throw away the rest of the
        // backup. A wholesale failure (no folder wrote at all) is surfaced after the loop instead.
        match tokio::task::spawn_blocking(move || std::fs::write(&path, &bytes)).await {
            Ok(Ok(())) => {
                wrote_any_folder = true;
                exported += written;
                text_only += folder_text_only;
            }
            _ => continue,
        }
    }
    // We only got here past the non-empty pre-check, so folders had mail. If *nothing* landed on disk it
    // wasn't an empty account — it was a real failure (a full or read-only destination), and reporting a
    // calm "no mail to export" would be a lie. Surface it.
    if !wrote_any_folder {
        return Err("Couldn't write the export files.".to_owned());
    }
    Ok(Some(crate::dto::ExportSummary {
        exported,
        text_only,
    }))
}

/// Assemble one folder's mbox for export (SEC-4): pull its messages' raw originals from the server
/// (best-effort — offline yields an empty map and the build falls back to reconstruction), then frame
/// them. Returns `(bytes, written, text_only, reachable)`. Shared by [`export_folder`] and
/// [`export_account`].
async fn folder_mbox_complete(
    st: AppState,
    folder_id: i64,
    folder_name: String,
    try_fetch: bool,
) -> Result<(Vec<u8>, i64, i64, bool), String> {
    let (account_id, uids, uidvalidity) = with_store(st.clone(), move |store| {
        Ok((
            store
                .account_for_folder(folder_id)
                .map_err(|_| "Couldn't read the folder to export.".to_owned())?,
            store
                .folder_uids(folder_id)
                .map_err(|_| "Couldn't read the folder to export.".to_owned())?,
            store
                .folder_uidvalidity(folder_id)
                .map_err(|_| "Couldn't read the folder to export.".to_owned())?,
        ))
    })
    .await?;
    // Fetch the raw originals on a worker. Nothing to fetch (a purely local folder, or already offline)
    // ⇒ empty map, still "reachable" — we didn't try, so the caller mustn't read it as an offline signal.
    let (raws, reachable) = match account_id {
        Some(account_id) if try_fetch && !uids.is_empty() => {
            let (db, secrets) = (st.db_path.clone(), st.secrets.clone());
            let fetched = tokio::task::spawn_blocking(move || {
                geleit_engine::sync_actions::run_fetch_folder_raws(
                    &db,
                    &*secrets,
                    account_id,
                    &folder_name,
                    &uids,
                    uidvalidity,
                )
            })
            .await
            .map_err(|_| "The export task stopped unexpectedly.".to_owned())?;
            match fetched {
                Some(map) => (map, true),        // reached the server
                None => (HashMap::new(), false), // unreachable — degrade, and let the caller fail-fast
            }
        }
        _ => (HashMap::new(), true),
    };
    let (bytes, written, text_only) =
        with_store(st, move |store| build_folder_mbox(store, folder_id, raws)).await?;
    Ok((bytes, written, text_only, reachable))
}

/// Build a folder's mbox archive, oldest-first, snoozed mail included. Returns `(bytes, written,
/// text_only)`, where `text_only` counts messages framed from the stored envelope + body because their
/// raw original wasn't fetched — so the caller can tell the user how much of the backup is text-only.
/// Separated from the command so it's testable without the save dialog. A single message that can't be
/// read or built is **skipped**, not fatal — one odd message must not cost the user the whole folder.
///
/// `raws` maps a message's IMAP uid to its **true original bytes** just pulled from the server (SEC-4).
/// When present, that exact message is written — attachments and all, byte-for-byte as the server holds
/// it. When absent (offline, a local-only Saved message, or a uid the server no longer has), the message
/// is reconstructed from the stored envelope + body instead — complete when online, still a backup when
/// not.
fn build_folder_mbox(
    store: &Store,
    folder_id: i64,
    mut raws: std::collections::HashMap<u32, Vec<u8>>,
) -> Result<(Vec<u8>, i64, i64), String> {
    let ids = store
        .folder_message_ids(folder_id)
        .map_err(|_| "Couldn't read the folder to export.".to_owned())?;
    let mut out = Vec::new();
    let mut written = 0i64;
    let mut text_only = 0i64;
    for id in &ids {
        let Some(header) = store.header_by_id(*id).ok().flatten() else {
            continue;
        };
        // Prefer the true original pulled from the server; fall back to reconstructing from what's stored.
        // `remove` (not `get`) takes ownership so there's no clone, and frees each raw as it's written —
        // so peak memory is the mbox being built, not the mbox *plus* a full second copy of every raw.
        let eml = match header.uid.and_then(|u| raws.remove(&(u as u32))) {
            Some(raw) => raw,
            None => {
                let body = store.body_for(*id).ok().flatten();
                let Ok(eml) = geleit_engine::message::export_eml(&header, body.as_ref()) else {
                    continue; // couldn't rebuild this one — skip it rather than abort the whole export
                };
                text_only += 1; // reconstructed, not the raw original — attachments (if any) not included
                eml
            }
        };
        let sender = header
            .from_addr
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("MAILER-DAEMON");
        out.extend_from_slice(&geleit_engine::message::mbox_entry(
            sender,
            &mbox_when(header.date),
            &eml,
        ));
        written += 1;
    }
    Ok((out, written, text_only))
}

/// A message's date as an mbox `From `-line timestamp (asctime, UTC). Informational — mbox readers key
/// only on the leading `From `.
fn mbox_when(date: Option<i64>) -> String {
    date.and_then(|ts| chrono::DateTime::from_timestamp(ts, 0))
        .map_or_else(
            || "Thu Jan  1 00:00:00 1970".to_owned(),
            |d| d.format("%a %b %e %T %Y").to_string(),
        )
}

/// The bytes + a suggested filename of message `message_id`'s `index`-th attachment. The bytes aren't
/// stored locally, so this fetches the raw message from the server on demand (`BODY.PEEK[]`) and
/// extracts the part (READ-8). The reusable core shared by the desktop's save dialog
/// ([`save_attachment`]) and the web host's download route.
pub async fn attachment_bytes(
    state: &AppState,
    message_id: i64,
    index: usize,
) -> Result<(Vec<u8>, String), String> {
    let st = state.clone();
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
    let (fetched_name, bytes) = tokio::task::spawn_blocking(move || {
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
    Ok((bytes, default_name))
}

/// Save a message's `index`-th attachment to disk via a native save dialog (READ-8). Fetches the bytes
/// with [`attachment_bytes`], then asks where to write them. Returns whether a file was written
/// (`false` = the user cancelled the dialog).
pub async fn save_attachment(
    state: &AppState,
    message_id: i64,
    index: usize,
) -> Result<bool, String> {
    let (bytes, default_name) = attachment_bytes(state, message_id, index).await?;
    tokio::task::spawn_blocking(move || {
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

/// Stage uploaded attachment bytes to a private temp file and return its path — the web host's
/// counterpart to the native file picker ([`pick_files`]), since a browser has no local paths to hand
/// [`send_message`]. Files land in a temp dir the OS reclaims; the name is sanitized and made unique so
/// two uploads of the same filename don't collide. Only reached over the web host's `/upload` route.
pub fn stage_upload(name: &str, bytes: &[u8]) -> Result<String, String> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join("geleit-uploads");
    std::fs::create_dir_all(&dir).map_err(|_| "Couldn't stage the upload.".to_owned())?;
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let path = dir.join(format!("{stamp}-{seq}-{}", safe_attachment_filename(name)));
    std::fs::write(&path, bytes).map_err(|_| "Couldn't stage the upload.".to_owned())?;
    Ok(path.to_string_lossy().into_owned())
}

/// Open a `.eml` file from disk (READ-10): pick a file, parse it, store it in the account's local
/// **Saved** folder, and return the new message id so the UI can switch there and open it. Returns
/// `None` if the user cancelled. No network — the file is parsed and rendered like any synced mail.
pub async fn open_eml_file(state: &AppState, account_id: i64) -> Result<Option<i64>, String> {
    // Pick + read the file off the async runtime (dialog + disk are blocking).
    let bytes = tokio::task::spawn_blocking(pick_open_eml)
        .await
        .map_err(|_| "The file picker stopped unexpectedly.".to_owned())??;
    let Some(bytes) = bytes else {
        return Ok(None); // cancelled
    };
    with_store(state.clone(), move |store| {
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

/// The messages this account still owes the user a notification for — its **INBOX** only. The
/// scheduler sweeps nothing else, and mail a server-side rule filed straight into a folder is not what
/// "new mail" means to a person.
pub async fn pending_notifications(
    state: &AppState,
    account_id: i64,
    limit: i64,
) -> Result<Vec<geleit_store::PendingNotification>, String> {
    with_store(state.clone(), move |store| {
        let Some(inbox) = store
            .folders_for_account(account_id)
            .unwrap_or_default()
            .into_iter()
            .find(|f| f.name.eq_ignore_ascii_case("INBOX"))
        else {
            return Ok(Vec::new());
        };
        store
            .pending_notifications(inbox.id, limit)
            .map_err(|_| "Couldn't read your mailbox.".to_owned())
    })
    .await
}

/// How much this account's inbox owes, and the newest message it owes for.
pub async fn pending_summary(
    state: &AppState,
    account_id: i64,
) -> Result<(i64, Option<i64>), String> {
    with_store(state.clone(), move |store| {
        let Some(inbox) = inbox_of(store, account_id) else {
            return Ok((0, None));
        };
        store
            .pending_notification_summary(inbox)
            .map_err(|_| "Couldn't read your mailbox.".to_owned())
    })
    .await
}

/// Settle the inbox's debt up to `max_id` — the user has been told (or has decided they don't want to
/// be). Bounded by id so mail that arrived in the meantime keeps its own debt.
pub async fn settle(state: &AppState, account_id: i64, max_id: i64) -> Result<(), String> {
    with_store(state.clone(), move |store| {
        let Some(inbox) = inbox_of(store, account_id) else {
            return Ok(());
        };
        store
            .mark_notified_through(inbox, max_id)
            .map(|_| ())
            .map_err(|_| "Couldn't update your mailbox.".to_owned())
    })
    .await
}

/// The account's INBOX folder id, or `None` if it has none yet.
fn inbox_of(store: &Store, account_id: i64) -> Option<i64> {
    store
        .folders_for_account(account_id)
        .ok()?
        .into_iter()
        .find(|f| f.name.eq_ignore_ascii_case("INBOX"))
        .map(|f| f.id)
}

/// A boolean setting, read by the **host** (the scheduler) rather than the frontend. `default` is what
/// an unset key means — notifications are on unless the user has turned them off.
pub async fn bool_setting(state: &AppState, key: &str, default: bool) -> bool {
    let key = key.to_owned();
    with_store(state.clone(), move |store| {
        Ok(store
            .get_setting(&key)
            .ok()
            .flatten()
            .map_or(default, |v| v == "1" || v == "true"))
    })
    .await
    .unwrap_or(default)
}

/// A free-text setting, read by the host. `None` = unset (or unreadable).
pub async fn string_setting(state: &AppState, key: &str) -> Option<String> {
    let key = key.to_owned();
    with_store(state.clone(), move |store| {
        Ok(store.get_setting(&key).ok().flatten())
    })
    .await
    .ok()
    .flatten()
}

/// Write a setting, for tests that drive the scheduler directly.
#[doc(hidden)]
pub async fn set_setting_for_test(state: &AppState, key: &str, value: &str) {
    let (key, value) = (key.to_owned(), value.to_owned());
    with_store(state.clone(), move |store| {
        store.set_setting(&key, &value).map_err(|_| String::new())
    })
    .await
    .expect("set setting");
}

/// Set the window title's unread badge (NOTIF-3) from the store's truth.
///
/// Cheap and idempotent, so every caller can just fire it: the frontend after anything that changes
/// read state (opening a message, mark read/unread, a move, a delete), the scheduler after a sweep.
/// The count is the total unread across every account's INBOX — see [`Store::total_inbox_unread`] — and
/// the title text is pure ([`crate::dto::window_title`]).
pub async fn set_badge(shell: &dyn Shell, state: &AppState) {
    let unread = with_store(state.clone(), |store| {
        store.total_inbox_unread().map_err(|_| String::new())
    })
    .await
    .unwrap_or(0);
    let title = crate::dto::window_title(unread);
    // The host decides what "badge" means: the desktop shell sets the window title + tray tooltip; a
    // windowless web host relays the title to the frontend as an event. One source of truth either way.
    shell.set_badge(&title);
}

/// The frontend's hook for the badge: call after anything that changes what's unread.
pub async fn update_badge(shell: &dyn Shell, state: &AppState) -> Result<(), String> {
    set_badge(shell, state).await;
    Ok(())
}

/// Drain an account's durable flag-write-back queue (SYNC-5), awaiting the result. The scheduler calls
/// this every sweep and Refresh calls it too, so a read/star made while offline reaches the server the
/// next time we're online — the queue survives restarts because it's just the store's dirty rows.
pub async fn flush_flags(state: &AppState, account_id: i64) {
    let (db, secrets) = (state.db_path.clone(), state.secrets.clone());
    let _ = tokio::task::spawn_blocking(move || {
        geleit_engine::sync_actions::run_flush_flags(&db, &*secrets, account_id)
    })
    .await;
}

/// Every account's id, for the background scheduler's sweep.
pub async fn account_ids(state: &AppState) -> Result<Vec<i64>, String> {
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
pub async fn sync_folder_once(
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
    tokio::task::spawn_blocking(move || {
        geleit_engine::sync_actions::run_refresh(&db, &*secrets, account_id, &folder)
    })
    .await
    .map_err(|_| "The sync task stopped unexpectedly.".to_owned())?
}

/// Refresh an account's folder: sync the folder list + the current folder's recent envelopes, then
/// backfill older mail in the background, emitting `sync-progress` events as batches land (P1 — the
/// UI never blocks; feedback streams instead). Returns when the *recent* sync is done; the backfill
/// keeps running and emitting. A network failure is reported calmly and leaves local mail untouched.
pub async fn refresh(
    shell: Arc<dyn Shell>,
    state: &AppState,
    account_id: i64,
    folder: String,
) -> Result<(), String> {
    let st = state.clone();
    let (db, secrets) = (state.db_path.clone(), state.secrets.clone());

    // Phase 1 — recent mail. Await this so the caller can re-list once it's in. Behind the folder's
    // sync lock, so a background sync of the same folder can't run alongside it (see `sync_lock`);
    // if one is already running, this waits for it and then syncs again — finding nothing new, which
    // is exactly right.
    sync_folder_once(&st, account_id, &folder).await?;
    // The user **asked** for this mail and is looking at the list it just landed in — so they have been
    // told, and a popup about it two minutes later would be the app interrupting them about something
    // already on their screen (P3). Settle the debt rather than announce it.
    //
    // (The old diff-based signal couldn't do this: Refresh's own diff consumed it. Making "told" a
    // durable fact is what created the possibility of telling someone twice.)
    if folder.eq_ignore_ascii_case("INBOX") {
        if let Ok((_, Some(max_id))) = pending_summary(&st, account_id).await {
            let _ = settle(&st, account_id, max_id).await;
        }
    }
    flush_flags(&st, account_id).await; // push any queued read/star changes now that we're online
    flush_outbox(&st, account_id).await; // and send anything waiting in the outbox
    set_badge(shell.as_ref(), &st).await;
    // That worked, so we're online — which is the one thing the background scheduler can't know while
    // it sits in a backed-off sleep. Wake it: it resets and sweeps the other accounts at once, rather
    // than leaving their mail up to half an hour stale after a laptop comes back from a night off.
    st.wake_sync().notify_one(); // stored permit: never lost, even if the scheduler is mid-sweep

    // Phase 2 — backfill older mail in the background, streaming progress. Detached: it may outlive
    // the command, and the UI shouldn't wait on it.
    std::thread::spawn(move || {
        // If the background backfill worker is already on this folder, don't download it a second time —
        // it'll finish. Close the UI's progress strip cleanly and step aside.
        if !st.try_begin_backfill(account_id, &folder) {
            shell.emit("sync-progress", serde_json::json!(-1));
            return;
        }
        // A drop guard releases the claim **and** emits the completion sentinel no matter how the thread
        // leaves — including a panic — so the claim can't leak and the UI's progress strip can never get
        // stuck. `-1` = finished cleanly, `-2` = it stopped early (the UI shows a calm "will resume"
        // note, S9.4-4).
        struct Done {
            st: AppState,
            account_id: i64,
            folder: String,
            shell: Arc<dyn Shell>,
            code: i64,
        }
        impl Drop for Done {
            fn drop(&mut self) {
                self.st.end_backfill(self.account_id, &self.folder);
                self.shell
                    .emit("sync-progress", serde_json::json!(self.code));
            }
        }
        let mut done = Done {
            st: st.clone(),
            account_id,
            folder: folder.clone(),
            shell: shell.clone(),
            code: -2,
        };

        let mut emit = |count: usize| {
            shell.emit("sync-progress", serde_json::json!(count as i64));
        };
        if geleit_engine::sync_actions::run_backfill(
            &db, &*secrets, account_id, &folder, 200, &mut emit,
        )
        .is_ok()
        {
            done.code = -1; // clean finish
        }
        // `done` drops here (or on a panic unwinding through this scope), releasing + emitting.
    });
    Ok(())
}

/// The persisted theme (`"dark"` / `"light"`), or `None` if the user has never chosen one.
///
/// The store is the source of truth — the same `setting` row the Slint app writes — so a user's
/// choice survives the M9 migration instead of silently reverting. `index.html` paints an *optimistic*
/// theme from `localStorage` before first paint (it cannot await IPC and still be instant); the app
/// reconciles against this on mount. The settings UI itself is S9.6.
pub async fn theme(state: &AppState) -> Result<Option<String>, String> {
    with_store(state.clone(), |store| {
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
pub async fn dev_open_message() -> Option<i64> {
    std::env::var("GELEIT_OPEN").ok()?.parse().ok()
}

/// Dev/test seam, debug builds only: `GELEIT_IMAGES=1` opts the auto-opened message in to remote
/// images, so the PRIV-2 path can be screenshot-verified without a click. Never in release.
#[cfg(debug_assertions)]
pub async fn dev_load_images() -> bool {
    std::env::var("GELEIT_IMAGES").is_ok_and(|v| v == "1")
}

/// Dev/test seam, debug builds only: `GELEIT_COMPOSE=new|reply|reply_all|forward` opens the compose
/// overlay on boot so it can be screenshot-verified without a click. Never in release.
#[cfg(debug_assertions)]
pub async fn dev_compose() -> Option<String> {
    std::env::var("GELEIT_COMPOSE").ok()
}

/// Dev/test seam, debug builds only: `GELEIT_UNIFIED=1` opens the merged "All inboxes" view on boot
/// so it can be screenshot-verified without a click. Never in release.
#[cfg(debug_assertions)]
pub async fn dev_unified() -> bool {
    std::env::var("GELEIT_UNIFIED").is_ok_and(|v| v == "1")
}

/// Dev/test seam, debug builds only: `GELEIT_SETUP=1` opens the add-account overlay on boot. Never
/// in release.
#[cfg(debug_assertions)]
pub async fn dev_setup() -> bool {
    std::env::var("GELEIT_SETUP").is_ok_and(|v| v == "1")
}

/// Dev/test seam, debug builds only: `GELEIT_SETTINGS=1` opens the Settings window on boot, or
/// `GELEIT_SETTINGS=<tab>` (accounts|general|appearance|privacy|notifications) opens it on that tab.
/// Never in release.
#[cfg(debug_assertions)]
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
pub async fn dev_search() -> Option<String> {
    std::env::var("GELEIT_SEARCH").ok()
}

/// Dev/test seam, debug builds only: `GELEIT_TRASH=empty|delete` opens the irreversible-delete confirm
/// dialog on boot (there's no click injection for the danger dialogs otherwise). Never in release.
#[cfg(debug_assertions)]
pub async fn dev_trash() -> Option<String> {
    std::env::var("GELEIT_TRASH").ok()
}

/// Dev/test seam, debug builds only: with `GELEIT_COMPOSE=new`, `GELEIT_TO=<text>` pre-fills the To
/// input on boot so the address-autocomplete dropdown can be screenshotted. Never in release.
#[cfg(debug_assertions)]
pub async fn dev_compose_to() -> Option<String> {
    std::env::var("GELEIT_TO").ok()
}

/// Dev/test seam, debug builds only: `GELEIT_DRAFTS=1` opens the Drafts list on boot. Never in release.
#[cfg(debug_assertions)]
pub async fn dev_drafts() -> bool {
    std::env::var("GELEIT_DRAFTS").is_ok_and(|v| v == "1")
}

/// Dev/test seam, debug builds only: `GELEIT_RESUME=1` resumes the newest draft on boot (opens the
/// composer with its content + materialised attachments). Never in release.
#[cfg(debug_assertions)]
pub async fn dev_resume() -> bool {
    std::env::var("GELEIT_RESUME").is_ok_and(|v| v == "1")
}

/// Dev/test seam, debug builds only: `GELEIT_SELECT=<id,id,…>` pre-selects those message rows on boot
/// so the multi-select bulk bar can be screenshotted. Never in release.
#[cfg(debug_assertions)]
pub async fn dev_select() -> Option<String> {
    std::env::var("GELEIT_SELECT").ok()
}

/// Dev/test seam, debug builds only: `GELEIT_FOLDER=new` opens the New-folder dialog on boot;
/// `GELEIT_FOLDER=menu` opens the first user folder's ⋯ (Rename/Delete) menu. Never in release.
#[cfg(debug_assertions)]
pub async fn dev_folder() -> Option<String> {
    std::env::var("GELEIT_FOLDER").ok()
}

#[cfg(test)]
mod tests {
    use super::{
        build_folder_mbox, materialize_draft_attachments, read_attachments, server_drafts, AppState,
    };
    use geleit_platform::secret::InMemorySecretStore;
    use geleit_store::{DraftContent, NewMessage, Store};

    #[test]
    fn backfill_is_single_flight_per_account_and_folder() {
        // The background backfill worker and a user Refresh both drive `run_backfill`; the guard makes one
        // of them step aside so a folder isn't downloaded twice. Distinct folders (and accounts) are
        // independent, and the claim is reusable once released.
        let state = AppState::new(
            "unused.db".to_owned(),
            std::sync::Arc::new(InMemorySecretStore::new()),
        );
        assert!(state.try_begin_backfill(1, "INBOX"), "first claim runs it");
        assert!(
            !state.try_begin_backfill(1, "INBOX"),
            "second is a no-op — already running"
        );
        assert!(
            state.try_begin_backfill(1, "Archive"),
            "a different folder is independent"
        );
        assert!(
            state.try_begin_backfill(2, "INBOX"),
            "a different account is independent"
        );
        state.end_backfill(1, "INBOX");
        assert!(
            state.try_begin_backfill(1, "INBOX"),
            "after it finishes, it can run again"
        );
    }

    #[test]
    fn idle_watch_is_claimed_once_and_the_pointer_check_protects_a_reused_id() {
        // The dedup behind instant-IDLE-for-new-accounts: a second claim must NOT start a second watcher
        // (double connections/wakes). And because SQLite reuses a removed account's id, a departing
        // watcher must free the slot ONLY if it still owns it — never evict the successor that took the id.
        let state = AppState::new(
            "unused.db".to_owned(),
            std::sync::Arc::new(InMemorySecretStore::new()),
        );
        let first = state
            .claim_idle_watch(7)
            .expect("first claim starts the watcher");
        assert!(
            state.claim_idle_watch(7).is_none(),
            "second claim is a no-op — already watched"
        );

        // Simulate remove (stop frees the slot) + re-add reusing id 7 → a *new* token owns the slot.
        state.stop_idle_watch(7);
        let second = state
            .claim_idle_watch(7)
            .expect("after stop, the reused id is claimable again");

        // The original watcher now exits and releases with ITS token — it must not evict the successor.
        state.release_idle_watch(7, &first);
        assert!(
            state.claim_idle_watch(7).is_none(),
            "the successor's watcher still holds the slot"
        );
        // The successor releasing its own token does free it.
        state.release_idle_watch(7, &second);
        assert!(
            state.claim_idle_watch(7).is_some(),
            "once the real owner releases, the slot is free"
        );
    }

    #[test]
    fn build_folder_mbox_frames_oldest_first_and_escapes_bodies() {
        let s = Store::open_in_memory().unwrap();
        let acc = s.add_account("me@x.com", None).unwrap();
        let inbox = s.upsert_folder(acc, "INBOX").unwrap();
        let m1 = s
            .upsert_message(
                acc,
                inbox,
                &NewMessage {
                    uid: Some(1),
                    date: Some(100),
                    subject: Some("First".into()),
                    from_addr: Some("alice@x.com".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        s.store_body(m1, Some("Hello from Alice."), None, Some("Hello"), false)
            .unwrap();
        let m2 = s
            .upsert_message(
                acc,
                inbox,
                &NewMessage {
                    uid: Some(2),
                    date: Some(200),
                    subject: Some("Second".into()),
                    from_addr: Some("bob@x.com".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        // A body line that begins `From ` — mbox must escape it so it isn't read as a separator.
        s.store_body(m2, Some("From the desk of Bob."), None, Some("From"), false)
            .unwrap();

        let (mbox, count, text_only) =
            build_folder_mbox(&s, inbox, std::collections::HashMap::new()).unwrap();
        assert_eq!(count, 2);
        assert_eq!(
            text_only, 2,
            "no raws supplied → both reconstructed (text-only)"
        );
        let text = String::from_utf8(mbox).unwrap();

        // Exactly one `From ` separator per message (line-start; header `From:` has a colon, not a space).
        let separators = usize::from(text.starts_with("From ")) + text.matches("\nFrom ").count();
        assert_eq!(separators, 2, "one separator per message:\n{text}");
        assert!(
            text.contains("From alice@x.com "),
            "sender in the separator"
        );
        // Oldest-first.
        assert!(
            text.find("First").unwrap() < text.find("Second").unwrap(),
            "oldest-first"
        );
        // Bob's body line is escaped, so it can't be mistaken for the next record.
        assert!(
            text.contains(">From the desk of Bob."),
            "body From-line escaped:\n{text}"
        );
    }

    #[test]
    fn build_folder_mbox_prefers_the_servers_raw_original_when_present() {
        // SEC-4 completeness: when a message's true raw bytes are supplied (fetched from the server, so
        // attachments are included), the export must write THOSE verbatim rather than the envelope+body
        // reconstruction — and still fall back to reconstruction for a message with no raw supplied.
        let s = Store::open_in_memory().unwrap();
        let acc = s.add_account("me@x.com", None).unwrap();
        let inbox = s.upsert_folder(acc, "INBOX").unwrap();
        let with_att = s
            .upsert_message(
                acc,
                inbox,
                &NewMessage {
                    uid: Some(7),
                    date: Some(100),
                    subject: Some("Has attachment".into()),
                    from_addr: Some("alice@x.com".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        // The store only ever held the parsed body text — never the attachment bytes.
        s.store_body(with_att, Some("see attached"), None, Some("see"), true)
            .unwrap();
        let plain = s
            .upsert_message(
                acc,
                inbox,
                &NewMessage {
                    uid: Some(8),
                    date: Some(200),
                    subject: Some("No raw fetched".into()),
                    from_addr: Some("bob@x.com".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        s.store_body(plain, Some("just text"), None, Some("just"), false)
            .unwrap();

        // The raw original the server would return for uid 7 — carrying the attachment the store lacks.
        let raw = b"Subject: Has attachment\r\n\r\nbody\r\n--BOUNDARY\r\n\
                    Content-Disposition: attachment; filename=\"report.pdf\"\r\n\r\nPDFBYTES\r\n"
            .to_vec();
        let mut raws = std::collections::HashMap::new();
        raws.insert(7u32, raw);

        let (mbox, count, text_only) = build_folder_mbox(&s, inbox, raws).unwrap();
        assert_eq!(count, 2);
        assert_eq!(
            text_only, 1,
            "uid 7 used its raw; only uid 8 fell back to reconstruction"
        );
        let text = String::from_utf8(mbox).unwrap();
        // uid 7 was written from the raw original — the attachment part is present.
        assert!(
            text.contains("filename=\"report.pdf\"") && text.contains("PDFBYTES"),
            "the server's raw original (with the attachment) must be exported verbatim:\n{text}"
        );
        // uid 8 had no raw supplied → reconstructed from the stored body.
        assert!(
            text.contains("just text"),
            "a message with no fetched raw falls back to reconstruction:\n{text}"
        );
    }

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
