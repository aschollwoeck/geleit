//! The "Soft daylight" three-pane desktop client (design-overhaul slice): folder rail · message list
//! · reading pane, plus compose / settings / add-account windows. Mail HTML is never rendered in this
//! document — it is confined to a sandboxed `mail://` iframe (ADR-0012).
use crate::api::{
    self, Account, AccountForm, ComposeDraft, DraftSummary, Folder, Message, MessageBody,
    ResumedDraft,
};
use crate::icons::{self, icon};
use crate::view::{
    all_selected, elide, format_date, is_protected_folder, is_trash_folder, merge_addrs, range_ids,
    rank_suggestions, split_addrs,
};
use leptos::either::Either;
use leptos::prelude::*;
use std::collections::HashSet;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::JsCast;

const PAGE: i64 = 300;

/// One entry in the flattened message list: a day header or a message row.
enum ListRow {
    Header(&'static str),
    Msg(Message),
}

/// A pending irreversible Trash action awaiting the danger-confirm dialog.
#[derive(Clone, Copy)]
enum TrashAsk {
    Empty,
    DeleteOne(i64),
}

/// The New-folder / Rename-folder dialog state. `rename_from` is `Some(old_name)` in rename mode.
#[derive(Clone)]
struct FolderForm {
    rename_from: Option<String>,
    name: String,
}

/// A move (archive / delete / spam) that has been requested but not yet committed to the server —
/// the message(s) are hidden from the list and an "Undo" toast is shown; the server move only happens
/// when the toast's window elapses, so Undo is a pure local cancel that can never lose mail. Holds a
/// set of ids so one bulk-select action archives/deletes them all under a single Undo window.
#[derive(Clone)]
struct PendingMove {
    ids: Vec<i64>,
    to: MoveTo,
}

/// Where a deferred move is going. The distinction is load-bearing: the toolbar's Archive / Delete /
/// Spam name a **role** and GeleitMail has to find the folder (the server's `\Trash` flag, then the
/// name); the Move… menu names the **folder the user picked**, and there is nothing to work out.
///
/// They used to be one thing: every folder in the Move… menu was mapped onto one of four roles by
/// matching English words, and anything that matched none of them — every ordinary folder, and every
/// folder on a provider that isn't English — fell through to `inbox`. Picking "Work" filed the message
/// in the Inbox.
#[derive(Clone, PartialEq, Eq)]
enum MoveTo {
    Role(&'static str),
    Folder(String),
}

/// Whether a row shows as unread, from the two session sets (see `read_now` / `marked_unread`).
/// `was_seen` is the message's server state at load; an explicit "mark unread" overrides it.
fn is_unread(
    id: i64,
    was_seen: bool,
    read_now: RwSignal<HashSet<i64>>,
    marked_unread: RwSignal<HashSet<i64>>,
) -> bool {
    marked_unread.with(|s| s.contains(&id)) || (!was_seen && !read_now.with(|s| s.contains(&id)))
}

/// Wall-clock seconds (UTC).
fn now_secs() -> i64 {
    #[cfg(target_arch = "wasm32")]
    {
        (js_sys::Date::now() / 1000.0) as i64
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        0
    }
}

/// Seconds to add to a UTC timestamp to get **local** wall-clock time (per timestamp, so DST is right).
fn local_offset(ts: i64) -> i64 {
    #[cfg(target_arch = "wasm32")]
    {
        let d = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64((ts * 1000) as f64));
        -(d.get_timezone_offset() as i64) * 60
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = ts;
        0
    }
}

/// A message's date, formatted in the reader's local time.
fn local_date(ts: Option<i64>) -> String {
    let now = now_secs();
    match ts {
        Some(t) => format_date(Some(t + local_offset(t)), now + local_offset(now)),
        None => String::new(),
    }
}

/// Which day-group a message belongs to — Today / Yesterday / Earlier — in the reader's local time.
fn day_bucket(ts: Option<i64>) -> &'static str {
    let Some(ts) = ts else { return "Earlier" };
    let day = (ts + local_offset(ts)).div_euclid(86_400);
    let now = now_secs();
    let today = (now + local_offset(now)).div_euclid(86_400);
    match today - day {
        d if d <= 0 => "Today",
        1 => "Yesterday",
        _ => "Earlier",
    }
}

/// The CSS variable for an account's indicator dot, by its position in the account list.
fn account_color(idx: usize) -> &'static str {
    match idx % 3 {
        0 => "var(--account-1)",
        1 => "var(--account-2)",
        _ => "var(--account-3)",
    }
}

/// Set the document theme and remember it (so the next launch paints it before first paint).
fn apply_theme(theme: &str) {
    #[cfg(target_arch = "wasm32")]
    {
        let Some(win) = web_sys::window() else { return };
        if let Some(root) = win.document().and_then(|d| d.document_element()) {
            let _ = root.set_attribute("data-theme", theme);
        }
        if let Ok(Some(storage)) = win.local_storage() {
            let _ = storage.set_item("geleit-theme", theme);
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = theme;
}

/// Whether the document is currently dark (the `data-theme` early.js painted).
fn document_is_dark() -> bool {
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.document_element())
            .and_then(|e| e.get_attribute("data-theme"))
            .as_deref()
            == Some("dark")
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        false
    }
}

/// Known-provider IMAP/SMTP servers, pre-filled in the add-account wizard. `(imap, imap_port, smtp,
/// smtp_port, starttls)`.
fn provider_servers(
    name: &str,
) -> Option<(&'static str, &'static str, &'static str, &'static str, bool)> {
    Some(match name {
        "Gmail" => ("imap.gmail.com", "993", "smtp.gmail.com", "465", false),
        "Outlook" => (
            "outlook.office365.com",
            "993",
            "smtp.office365.com",
            "587",
            true,
        ),
        "GMX" => ("imap.gmx.net", "993", "mail.gmx.net", "587", true),
        "Web.de" => ("imap.web.de", "993", "smtp.web.de", "587", true),
        "Yahoo" => (
            "imap.mail.yahoo.com",
            "993",
            "smtp.mail.yahoo.com",
            "465",
            false,
        ),
        "iCloud" => ("imap.mail.me.com", "993", "smtp.mail.me.com", "587", true),
        _ => return None,
    })
}

#[component]
pub fn App() -> impl IntoView {
    // ---- data ----
    let accounts = RwSignal::new(Vec::<Account>::new());
    let account = RwSignal::new(Option::<i64>::None);
    let unified = RwSignal::new(false); // the merged "All inboxes" view (spans every account)
    let folders = RwSignal::new(Vec::<Folder>::new());
    let selected_folder = RwSignal::new(Option::<i64>::None);
    let messages = RwSignal::new(Vec::<Message>::new());
    let open = RwSignal::new(Option::<MessageBody>::None);
    // The open message's star state, captured when it opens — the body DTO doesn't carry it, and the
    // message may later leave `messages` (e.g. after clearing a search), so the reading pane can't
    // rely on looking it up there.
    let open_flagged = RwSignal::new(false);
    let error = RwSignal::new(Option::<String>::None);
    let loaded = RwSignal::new(false);
    // Read/unread tracked this session in small sets, kept apart from `messages` so toggling one
    // doesn't clone the whole list. `read_now`: opened this session. `marked_unread`: explicitly
    // set unread (works even for a message that arrived already-read).
    let read_now = RwSignal::new(HashSet::<i64>::new());
    let marked_unread = RwSignal::new(HashSet::<i64>::new());
    let selected = RwSignal::new(HashSet::<i64>::new()); // messages picked for a bulk action
    let select_anchor = RwSignal::new(Option::<i64>::None); // last-toggled row, for shift-click ranges
                                                            // Only the newest request may write the list (guards against a stale folder/search reply clobbering).
    let request = RwSignal::new(0u64);
    // ---- reading ----
    let load_images = RwSignal::new(false); // PRIV-2: per message
                                            // ---- sync ----
    let refreshing = RwSignal::new(false);
    let catchup = RwSignal::new(Option::<i64>::None);
    // ---- chrome / overlays ----
    let rail_collapsed = RwSignal::new(false);
    let acct_menu = RwSignal::new(false);
    let move_menu = RwSignal::new(false);
    let compose = RwSignal::new(Option::<ComposeDraft>::None);
    let current_draft_id = RwSignal::new(Option::<i64>::None); // the saved draft being edited, if any
                                                               // The provider's draft being continued (a message id). Saving or sending removes it from the
                                                               // server: the draft you edited is the draft you now have, and leaving the original there would put
                                                               // it straight back in the list as a second row.
    let resumed_server = RwSignal::new(Option::<i64>::None);
    let formatted_ask = RwSignal::new(Option::<i64>::None); // "continuing this drops its formatting"
                                                            // Deleting a draft that's on the provider is irreversible and it may be the ONLY copy — so it asks,
                                                            // like every other permanent action here. A local draft is still a one-click delete (it's ours,
                                                            // and it's the one the trash icon has always meant).
    let draft_del = RwSignal::new(Option::<i64>::None);
    let draft_busy = RwSignal::new(false); // fetching a provider draft (its attachments) — one at a time
    let drafts_loading = RwSignal::new(false); // …and the first look at the provider's Drafts folder
    let drafts_open = RwSignal::new(false); // list pane shows saved drafts instead of a folder
    let drafts = RwSignal::new(Vec::<DraftSummary>::new());
    let to_input = RwSignal::new(String::new()); // in-progress recipient text (not yet a chip)
    let cc_input = RwSignal::new(String::new());
    let attach_paths = RwSignal::new(Vec::<String>::new()); // files attached to the current draft
    let md_on = RwSignal::new(false); // send the current draft as Markdown (text + HTML)
                                      // Autocomplete matches per recipient field, held at App scope so the global Escape handler can
                                      // close the dropdown before it would otherwise discard the whole draft.
    let to_suggest = RwSignal::new(Vec::<String>::new());
    let cc_suggest = RwSignal::new(Vec::<String>::new());
    let sending = RwSignal::new(false);
    let query = RwSignal::new(String::new());
    let search_open = RwSignal::new(false);
    let settings_open = RwSignal::new(false);
    let settings_tab = RwSignal::new("accounts".to_string());
    let confirm = RwSignal::new(Option::<(i64, String)>::None); // (account_id, email) to remove
    let trash_ask = RwSignal::new(Option::<TrashAsk>::None); // irreversible Trash action to confirm
    let folder_form = RwSignal::new(Option::<FolderForm>::None); // New/Rename folder dialog
    let folder_del = RwSignal::new(Option::<(i64, String)>::None); // (id, name) folder to delete
    let folder_menu = RwSignal::new(Option::<i64>::None); // which folder's ⋯ menu is open
    let toast = RwSignal::new(Option::<String>::None);
    let pending = RwSignal::new(Option::<PendingMove>::None); // move awaiting its Undo window
                                                              // Handle of the running toast/commit timer, so a newer toast or an Undo can cancel it.
    #[cfg(target_arch = "wasm32")]
    let toast_timer = RwSignal::new(Option::<i32>::None);
    let dark = RwSignal::new(document_is_dark());
    // settings-backed prefs
    let block_remote = RwSignal::new(true);
    let sync_drafts = RwSignal::new(false); // opt-in: mirror drafts to the server (default OFF, P2)
    let mark_read = RwSignal::new(true);
    let notify = RwSignal::new(true); // a mail client that never tells you about mail is a strange one
    let outbox = RwSignal::new((0i64, 0i64)); // (queued, failed) — the outbox indicator (SEND-10)
    let outbox_open = RwSignal::new(false); // the middle pane shows the outbox instead of a folder
    let outbox_list = RwSignal::new(Vec::<api::OutboxItem>::new());
    let quiet_on = RwSignal::new(false); // quiet hours: silent, but the mail is still owed a mention
    let quiet_bad = RwSignal::new(false); // a window the host would throw away — tell them, don't store it
    let quiet_from = RwSignal::new("22:00".to_owned());
    let quiet_to = RwSignal::new("07:00".to_owned());
    let notify_accounts = RwSignal::new(std::collections::HashMap::<i64, bool>::new());
    let sig_text = RwSignal::new(String::new());
    // ---- wizard ----
    let wiz = RwSignal::new(Option::<AccountForm>::None); // Some while open (manual step)
    let wiz_note = RwSignal::new(Option::<String>::None); // provider-prefilled note
    let connecting = RwSignal::new(false);

    // Backfill progress (-1 ok, -2 stopped) → clear the strip and re-list the current view.
    api::on_sync_progress(move |n| {
        if n < 0 {
            catchup.set(None);
            if n == -2 {
                error.set(Some(
                    "Couldn't finish catching up — will resume next refresh.".into(),
                ));
            }
            let q = query.get_untracked();
            if unified.get_untracked() {
                // Merged view: re-list across all inboxes (or re-run the cross-account search).
                let epoch = request.get_untracked() + 1;
                request.set(epoch);
                leptos::task::spawn_local(async move {
                    let result = if q.trim().is_empty() {
                        api::list_all_messages(PAGE).await
                    } else {
                        api::search_all(&q).await
                    };
                    if let Ok(m) = result {
                        if request.get_untracked() == epoch {
                            messages.set(m);
                        }
                    }
                });
            } else if let Some(fid) = selected_folder.get_untracked() {
                let epoch = request.get_untracked() + 1;
                request.set(epoch);
                leptos::task::spawn_local(async move {
                    let result = if q.trim().is_empty() {
                        api::list_messages(fid, PAGE).await
                    } else if let Some(aid) = account.get_untracked() {
                        api::search(aid, &q).await
                    } else {
                        return;
                    };
                    if let Ok(m) = result {
                        if request.get_untracked() == epoch {
                            messages.set(m);
                        }
                    }
                });
            }
        } else {
            catchup.set(Some(n));
        }
    });

    // Recompute the window's unread badge from the store (NOTIF-3). Fire-and-forget: it lags the
    // optimistic on-screen change by a store round-trip, which the badge — a taskbar glance, not a live
    // counter — can well afford. Call it after anything that changes what's unread.
    let bump_badge = move || {
        leptos::task::spawn_local(async move {
            let _ = api::update_badge().await;
        });
    };

    // Refresh the outbox indicator from the store (SEND-10). Cheap; call it after a send and whenever a
    // sweep may have drained it.
    let refresh_outbox = move || {
        leptos::task::spawn_local(async move {
            if let Ok(counts) = api::outbox_status().await {
                outbox.set(counts);
            }
        });
    };
    // Load the outbox list into the pane (the outbox is app-wide, not per-account).
    let load_outbox = move || {
        leptos::task::spawn_local(async move {
            if let Ok(list) = api::list_outbox().await {
                outbox_list.set(list);
            }
        });
    };
    // Show the outbox in the middle pane (reached by clicking the indicator).
    let open_outbox = move || {
        unified.set(false);
        drafts_open.set(false);
        outbox_open.set(true);
        selected.set(HashSet::new());
        selected_folder.set(None);
        open.set(None);
        search_open.set(false);
        query.set(String::new());
        acct_menu.set(false);
        load_outbox();
    };
    // Retry a failed send (re-queue + flush) or discard one; then refresh the pane + the indicator.
    let retry_outbox_msg = move |id: i64| {
        outbox_list.update(|l| l.retain(|o| o.id != id)); // it leaves the failed list immediately
        leptos::task::spawn_local(async move {
            if let Err(e) = api::retry_outbox(id).await {
                error.set(Some(e));
            }
            load_outbox();
            refresh_outbox();
        });
    };
    let discard_outbox_msg = move |id: i64| {
        outbox_list.update(|l| l.retain(|o| o.id != id));
        leptos::task::spawn_local(async move {
            if let Err(e) = api::discard_outbox(id).await {
                error.set(Some(e));
            }
            load_outbox(); // reconcile the pane if the discard didn't land
            refresh_outbox();
        });
    };

    // The background scheduler found new mail (NOTIF-1). Slip it into the list quietly — no toast, no
    // jump: it just appears, with its unread dot, the way mail should. The re-list goes through the
    // `request` epoch like every other one, so it can never clobber a search the user is mid-way
    // through typing or a folder they just switched to. A message that's open stays open.
    api::on_mail_arrived(move |_n| {
        let q = query.get_untracked();
        if !q.trim().is_empty() {
            return; // they're searching — don't yank the results out from under them
        }
        if drafts_open.get_untracked() {
            return; // the drafts pane isn't a mail list
        }
        // (No bump here: the scheduler set the badge host-side before emitting this event.)
        refresh_outbox(); // a sweep may have sent queued mail or hit a rejection
        let epoch = request.get_untracked() + 1;
        request.set(epoch);
        let is_unified = unified.get_untracked();
        let fid = selected_folder.get_untracked();
        let aid = account.get_untracked();
        leptos::task::spawn_local(async move {
            let result = if is_unified {
                api::list_all_messages(PAGE).await
            } else if let Some(fid) = fid {
                api::list_messages(fid, PAGE).await
            } else {
                return;
            };
            if let Ok(m) = result {
                if request.get_untracked() == epoch {
                    messages.set(m);
                }
            }
            // Keep the rail's unread counts honest.
            if let Some(aid) = aid {
                if let Ok(fs) = api::list_folders(aid).await {
                    if account.get_untracked() == Some(aid) {
                        folders.set(fs);
                    }
                }
            }
        });
    });

    // Boot: accounts → the first account's folders + messages; load persisted prefs.
    Effect::new(move |_| {
        leptos::task::spawn_local(async move {
            block_remote.set(
                api::get_bool_setting("block_remote")
                    .await
                    .ok()
                    .flatten()
                    .unwrap_or(true),
            );
            sync_drafts.set(
                api::get_bool_setting("sync_drafts")
                    .await
                    .ok()
                    .flatten()
                    .unwrap_or(false), // drafts stay on this device unless you ask otherwise
            );
            mark_read.set(
                api::get_bool_setting("mark_read")
                    .await
                    .ok()
                    .flatten()
                    .unwrap_or(true),
            );
            notify.set(
                api::get_bool_setting("notify")
                    .await
                    .ok()
                    .flatten()
                    .unwrap_or(true), // on unless the user has said otherwise
            );
            // Quiet hours are one string ("22:00-07:00"); unset or empty = off.
            if let Ok(Some(raw)) = api::get_setting("quiet_hours").await {
                if let Some((from, to)) = raw.split_once('-') {
                    quiet_on.set(true);
                    quiet_from.set(from.trim().to_owned());
                    quiet_to.set(to.trim().to_owned());
                }
            }
            if let Ok(Some(t)) = api::theme().await {
                dark.set(t == "dark");
                apply_theme(&t);
            }
            match api::list_accounts().await {
                Ok(list) => match list.first() {
                    Some(a) => {
                        let aid = a.id;
                        accounts.set(list);
                        account.set(Some(aid));
                        if let Ok(s) = api::get_signature(aid).await {
                            sig_text.set(s);
                        }
                        load_folders(aid, folders, selected_folder, messages, error, request).await;
                        loaded.set(true);
                        bump_badge(); // the title is right from the first frame, not 30s later
                        refresh_outbox();
                    }
                    None => {
                        accounts.set(list);
                        loaded.set(true);
                    }
                },
                Err(e) => error.set(Some(e)),
            }
        });
    });

    // ---- handlers ----
    let choose_folder = move |id: i64| {
        unified.set(false);
        drafts_open.set(false);
        outbox_open.set(false);
        selected.set(HashSet::new());
        selected_folder.set(Some(id));
        open.set(None);
        query.set(String::new());
        search_open.set(false);
        messages.set(Vec::new());
        let epoch = request.get_untracked() + 1;
        request.set(epoch);
        leptos::task::spawn_local(async move {
            match api::list_messages(id, PAGE).await {
                Ok(m) if request.get_untracked() == epoch => messages.set(m),
                Ok(_) => {}
                Err(e) => error.set(Some(e)),
            }
        });
    };

    // Switch to the merged "All inboxes" view: every account's INBOX in one date-sorted list.
    let choose_unified = move || {
        unified.set(true);
        drafts_open.set(false);
        outbox_open.set(false);
        selected.set(HashSet::new());
        acct_menu.set(false);
        selected_folder.set(None);
        open.set(None);
        query.set(String::new());
        search_open.set(false);
        messages.set(Vec::new());
        let epoch = request.get_untracked() + 1;
        request.set(epoch);
        leptos::task::spawn_local(async move {
            match api::list_all_messages(PAGE).await {
                Ok(m) if request.get_untracked() == epoch => messages.set(m),
                Ok(_) => {}
                Err(e) => error.set(Some(e)),
            }
        });
    };

    let choose_message = move |id: i64| {
        load_images.set(!block_remote.get_untracked()); // block-off ⇒ show images by default
        move_menu.set(false);
        // In the merged view a row can belong to any account — adopt its account so a reply is sent
        // from the right mailbox and the reading-pane header shows the right one.
        if unified.get_untracked() {
            if let Some(acc) = messages
                .get_untracked()
                .iter()
                .find(|m| m.id == id)
                .map(|m| m.account)
            {
                account.set(Some(acc));
            }
        }
        // Capture the star state now, while the message is still in the loaded list.
        let flag = messages
            .get_untracked()
            .iter()
            .find(|m| m.id == id)
            .map(|m| m.flagged)
            .unwrap_or(false);
        let mr = mark_read.get_untracked(); // "mark as read when opened" preference
        leptos::task::spawn_local(async move {
            match api::open_message(id, mr).await {
                Ok(body) => {
                    open_flagged.set(flag);
                    open.set(Some(body));
                    // When the preference is on, mark it read this session and clear any earlier
                    // "mark unread". When off, leave the unread state exactly as it was.
                    if mr {
                        read_now.update(|s| {
                            s.insert(id);
                        });
                        marked_unread.update(|s| {
                            s.remove(&id);
                        });
                        bump_badge();
                    }
                }
                Err(e) => error.set(Some(e)),
            }
        });
    };

    // Star / unstar the open message (ORG-4). Flip the captured `open_flagged` (authoritative even if
    // the message has since left the list), mirror it into the list row if present, then write back.
    let toggle_star = move || {
        let Some(id) = open.get().map(|b| b.id) else {
            return;
        };
        let now_on = !open_flagged.get_untracked();
        open_flagged.set(now_on);
        messages.update(|list| {
            if let Some(m) = list.iter_mut().find(|m| m.id == id) {
                m.flagged = now_on;
            }
        });
        leptos::task::spawn_local(async move {
            if let Err(e) = api::set_star(id, now_on).await {
                error.set(Some(e));
            }
        });
    };

    let mark_unread = move || {
        let Some(id) = open.get().map(|b| b.id) else {
            return;
        };
        // Unread is tracked in two small sets rather than by mutating the list: drop it from
        // read-this-session and add it to marked-unread. This also restores the dot for a message
        // that arrived already-read on the server (its `seen` snapshot alone couldn't express that).
        read_now.update(|s| {
            s.remove(&id);
        });
        marked_unread.update(|s| {
            s.insert(id);
        });
        leptos::task::spawn_local(async move {
            if let Err(e) = api::set_unread(id).await {
                error.set(Some(e));
            } else {
                bump_badge(); // only now the store says seen=0 — bumping earlier would read stale
            }
        });
    };

    // Commit the pending move to the server: drop the row locally and run the move. A server refusal
    // or failure restores the row (nothing is ever lost). Clears the pending state + toast.
    let commit_pending = move || {
        let Some(pm) = pending.get_untracked() else {
            return;
        };
        #[cfg(target_arch = "wasm32")]
        if let Some(h) = toast_timer.get_untracked() {
            if let Some(w) = web_sys::window() {
                w.clear_timeout_with_handle(h);
            }
        }
        let to = pm.to;
        // Keep only the affected rows so a failed move re-inserts *those* — not a whole stale snapshot
        // that could resurrect a message a concurrent (overlapping) commit has since removed.
        let saved: Vec<Message> = messages
            .get_untracked()
            .into_iter()
            .filter(|m| pm.ids.contains(&m.id))
            .collect();
        messages.update(|list| list.retain(|m| !pm.ids.contains(&m.id)));
        pending.set(None);
        toast.set(None);
        for id in pm.ids {
            // Each id keeps its own row for a precise re-insert on failure.
            let row = saved.iter().find(|m| m.id == id).cloned();
            let to = to.clone();
            leptos::task::spawn_local(async move {
                let restore = move |msg: String| {
                    if let Some(m) = row {
                        messages.update(|list| {
                            if !list.iter().any(|x| x.id == id) {
                                list.push(m);
                            }
                        });
                    }
                    error.set(Some(msg));
                };
                let done = match &to {
                    MoveTo::Role(role) => api::move_to_role(id, role).await,
                    MoveTo::Folder(name) => api::move_to_folder(id, name).await,
                };
                match done {
                    Ok(true) => bump_badge(), // one fewer (maybe unread) message in the inbox
                    Ok(false) => restore("This account has no folder for that.".to_owned()),
                    Err(e) => restore(e),
                }
            });
        }
    };

    // Cancel a pending move within its window: the row simply reappears (it was only hidden, never
    // touched on the server), so this can't lose mail.
    let undo_pending = move || {
        #[cfg(target_arch = "wasm32")]
        if let Some(h) = toast_timer.get_untracked() {
            if let Some(w) = web_sys::window() {
                w.clear_timeout_with_handle(h);
            }
        }
        pending.set(None);
        toast.set(None);
    };

    // Archive / trash / spam the open message. The move is *deferred*: the row is hidden and an Undo
    // toast is shown; the server move only runs when the toast window elapses (see the auto-dismiss
    // Effect), so Undo is a pure local cancel. Any earlier pending move is committed first.
    let move_open_to = move |to: MoveTo, verb: &'static str| {
        let Some(id) = open.get().map(|b| b.id) else {
            return;
        };
        commit_pending(); // only one move can be pending at a time
        open.set(None);
        move_menu.set(false);
        pending.set(Some(PendingMove { ids: vec![id], to }));
        toast.set(Some(verb.to_owned()));
    };
    // Archive / Delete / Spam: a role, which GeleitMail resolves to a folder.
    let move_open =
        move |role: &'static str, verb: &'static str| move_open_to(MoveTo::Role(role), verb);
    // Move…: the folder the user named. No resolving, no guessing.
    let move_open_folder = move |name: String| move_open_to(MoveTo::Folder(name), "Moved");

    // Archive / delete every selected message under one deferred Undo window (reuses the move machinery
    // above). Clears the selection immediately; the server moves run only when the toast elapses.
    let bulk_move = move |role: &'static str, verb: &'static str| {
        let ids: Vec<i64> = selected.get_untracked().into_iter().collect();
        if ids.is_empty() {
            return;
        }
        commit_pending(); // flush any earlier pending move before starting this one
        open.set(None);
        selected.set(HashSet::new());
        select_anchor.set(None);
        let toast_text = format!("{} {verb}", ids.len());
        pending.set(Some(PendingMove {
            ids,
            to: MoveTo::Role(role),
        }));
        toast.set(Some(toast_text));
    };

    // Mark every selected message unread — an immediate per-message write-back (like the single-row
    // action; there's no deferred Undo for read-state, matching the reading-pane "Unread").
    let bulk_mark_unread = move || {
        let ids: Vec<i64> = selected.get_untracked().into_iter().collect();
        if ids.is_empty() {
            return;
        }
        selected.set(HashSet::new());
        select_anchor.set(None);
        for id in &ids {
            marked_unread.update(|s| {
                s.insert(*id);
            });
            read_now.update(|s| {
                s.remove(id);
            });
        }
        let toast_text = format!("{} marked unread", ids.len());
        leptos::task::spawn_local(async move {
            for id in ids {
                if let Err(e) = api::set_unread(id).await {
                    error.set(Some(e));
                }
            }
            bump_badge(); // once every write has landed, so the count reflects all of them
        });
        toast.set(Some(toast_text));
    };

    // Mark every selected message read — immediate per-message write-back (the mirror of bulk unread).
    let bulk_mark_read = move || {
        let ids: Vec<i64> = selected.get_untracked().into_iter().collect();
        if ids.is_empty() {
            return;
        }
        selected.set(HashSet::new());
        select_anchor.set(None);
        for id in &ids {
            read_now.update(|s| {
                s.insert(*id);
            });
            marked_unread.update(|s| {
                s.remove(id);
            });
        }
        let toast_text = format!("{} marked read", ids.len());
        leptos::task::spawn_local(async move {
            for id in ids {
                if let Err(e) = api::set_read(id).await {
                    error.set(Some(e));
                }
            }
            bump_badge();
        });
        toast.set(Some(toast_text));
    };

    // Click a row's checkbox: shift-click extends a range from the anchor (last plain click); a plain
    // click toggles the one row and becomes the new anchor.
    let select_click = move |id: i64, shift: bool| {
        if shift {
            if let Some(anchor) = select_anchor.get_untracked() {
                // Range over the *visible* order: the list re-buckets into Today/Yesterday/Earlier
                // (and search returns rank order, not date order), so raw `messages` order would pick
                // the wrong span. Mirror the `rows()` bucketing and drop any pending-hidden rows.
                let hidden = pending.get_untracked().map(|p| p.ids).unwrap_or_default();
                let msgs = messages.get_untracked();
                let mut ordered: Vec<i64> = Vec::new();
                for bucket in ["Today", "Yesterday", "Earlier"] {
                    for m in &msgs {
                        if !hidden.contains(&m.id) && day_bucket(m.date) == bucket {
                            ordered.push(m.id);
                        }
                    }
                }
                let range = range_ids(&ordered, anchor, id);
                selected.update(|s| s.extend(range));
                return;
            }
        }
        selected.update(|s| {
            if !s.remove(&id) {
                s.insert(id);
            }
        });
        select_anchor.set(Some(id));
    };

    // Keyboard list navigation (j/k / arrows): open the message `delta` steps from the current one,
    // skipping the day headers. Hidden (pending-move) rows are excluded. Scrolls the row into view.
    #[cfg(target_arch = "wasm32")]
    let nav = move |delta: i32| {
        let hidden = pending.get_untracked().map(|p| p.ids).unwrap_or_default();
        let ids: Vec<i64> = messages
            .get_untracked()
            .iter()
            .map(|m| m.id)
            .filter(|id| !hidden.contains(id))
            .collect();
        if ids.is_empty() {
            return;
        }
        let cur = open.get_untracked().map(|b| b.id);
        let pos = cur.and_then(|c| ids.iter().position(|&x| x == c));
        let Some(next) = crate::view::nav_index(ids.len(), pos, delta) else {
            return;
        };
        let id = ids[next];
        choose_message(id);
        if let Some(el) = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.get_element_by_id(&format!("row-{id}")))
        {
            el.scroll_into_view_with_bool(false); // keep the row within the viewport
        }
    };

    let run_search = move |q: String| {
        query.set(q.clone());
        selected.set(HashSet::new()); // the visible set changes under search — drop the selection
        select_anchor.set(None);
        let is_unified = unified.get();
        // Per-account search needs a selected account; the merged view searches across all.
        if !is_unified && account.get().is_none() {
            return;
        }
        let aid = account.get();
        let epoch = request.get_untracked() + 1;
        request.set(epoch);
        leptos::task::spawn_local(async move {
            let result = if q.trim().is_empty() {
                // empty query → back to the underlying listing
                if is_unified {
                    api::list_all_messages(PAGE).await
                } else {
                    match selected_folder.get_untracked() {
                        Some(fid) => api::list_messages(fid, PAGE).await,
                        None => Ok(Vec::new()),
                    }
                }
            } else if is_unified {
                api::search_all(&q).await
            } else {
                api::search(aid.expect("checked above"), &q).await
            };
            match result {
                Ok(m) if request.get_untracked() == epoch => messages.set(m),
                Ok(_) => {}
                Err(e) => error.set(Some(e)),
            }
        });
    };

    let set_theme = move |to_dark: bool| {
        let next = if to_dark { "dark" } else { "light" };
        dark.set(to_dark);
        apply_theme(next);
        leptos::task::spawn_local(async move {
            let _ = api::set_theme(next).await;
        });
    };
    let toggle_theme = move || set_theme(!dark.get());

    // account switching
    let switch_account = move |aid: i64| {
        unified.set(false); // picking a specific account leaves the merged view
        drafts_open.set(false); // and leaves the drafts view (drafts are per-account)
        outbox_open.set(false); // …and the outbox pane, so the rail and the middle pane agree
        selected.set(HashSet::new());
        acct_menu.set(false);
        account.set(Some(aid));
        leptos::task::spawn_local(async move {
            if let Ok(s) = api::get_signature(aid).await {
                sig_text.set(s);
            }
            load_folders(aid, folders, selected_folder, messages, error, request).await;
        });
    };

    // wizard
    let open_wizard = move || {
        acct_menu.set(false);
        settings_open.set(false);
        wiz_note.set(None);
        wiz.set(Some(AccountForm {
            imap_port: "993".into(),
            smtp_port: "465".into(),
            ..AccountForm::default()
        }));
    };
    let pick_provider = move |name: &'static str| {
        if let Some((imap, iport, smtp, sport, starttls)) = provider_servers(name) {
            wiz.update(|w| {
                if let Some(w) = w {
                    w.imap_host = imap.into();
                    w.imap_port = iport.into();
                    w.smtp_host = smtp.into();
                    w.smtp_port = sport.into();
                    w.smtp_starttls = starttls;
                }
            });
            wiz_note.set(Some(format!(
                "{name} servers filled in — just add your password."
            )));
        }
    };
    let submit_wizard = move || {
        let Some(form) = wiz.get() else { return };
        if connecting.get() {
            return;
        }
        connecting.set(true);
        leptos::task::spawn_local(async move {
            match api::add_account(&form).await {
                Ok(aid) => {
                    wiz.set(None);
                    unified.set(false); // land on the new account, not a stale merged view
                    if let Ok(list) = api::list_accounts().await {
                        accounts.set(list);
                    }
                    account.set(Some(aid));
                    load_folders(aid, folders, selected_folder, messages, error, request).await;
                    loaded.set(true);
                    commit_pending(); // resolve any queued move before this confirmation toast
                    toast.set(Some("Account added".into()));
                }
                Err(e) => error.set(Some(e)),
            }
            connecting.set(false);
        });
    };

    // settings
    let open_settings = move || {
        acct_menu.set(false);
        settings_open.set(true);
    };
    let toggle_block = move || {
        let next = !block_remote.get();
        block_remote.set(next);
        leptos::task::spawn_local(async move {
            let _ = api::set_bool_setting("block_remote", next).await;
        });
    };
    let toggle_sync_drafts = move || {
        let next = !sync_drafts.get();
        sync_drafts.set(next);
        let aid = account.get();
        leptos::task::spawn_local(async move {
            let _ = api::set_bool_setting("sync_drafts", next).await;
            // Turning it OFF takes the drafts back off the server — not just "stop uploading new
            // ones", which would leave unsent content sitting there.
            if !next {
                if let Some(aid) = aid {
                    match api::purge_server_drafts(aid).await {
                        Ok(()) => toast.set(Some("Drafts removed from your provider".into())),
                        Err(e) => error.set(Some(e)),
                    }
                }
            }
        });
    };
    let toggle_mark = move || {
        let next = !mark_read.get();
        mark_read.set(next);
        leptos::task::spawn_local(async move {
            let _ = api::set_bool_setting("mark_read", next).await;
        });
    };
    let toggle_notify = move || {
        let next = !notify.get();
        notify.set(next);
        leptos::task::spawn_local(async move {
            let _ = api::set_bool_setting("notify", next).await;
        });
    };
    // Quiet hours are written as one string, and cleared to "" when switched off. The host treats
    // anything it can't parse as "no quiet hours" — never as "silent forever".
    let save_quiet = move || {
        let (from, to) = (quiet_from.get_untracked(), quiet_to.get_untracked());
        // A zero-length window is not a window: the host discards anything it can't parse as "no quiet
        // hours" (never "silent forever"), and the toggle would go on reading ON. The user asked for
        // silence and would have got the opposite, with no feedback. Say so instead.
        let bad = quiet_on.get_untracked() && (from.trim().is_empty() || from == to);
        quiet_bad.set(bad);
        if bad {
            return;
        }
        let value = if quiet_on.get_untracked() {
            format!("{from}-{to}")
        } else {
            String::new()
        };
        leptos::task::spawn_local(async move {
            let _ = api::set_setting("quiet_hours", &value).await;
        });
    };
    let toggle_quiet = move || {
        quiet_on.update(|q| *q = !*q);
        save_quiet();
    };
    // Per account, so one noisy mailbox doesn't cost the user the notifications of the other.
    let load_account_notify = move || {
        leptos::task::spawn_local(async move {
            let mut map = std::collections::HashMap::new();
            for a in accounts.get_untracked() {
                let on = api::get_bool_setting(&format!("notify_account_{}", a.id))
                    .await
                    .ok()
                    .flatten()
                    .unwrap_or(true);
                map.insert(a.id, on);
            }
            notify_accounts.set(map);
        });
    };
    // Whenever the settings window opens, read the per-account switches back (an account may have been
    // added since the last time).
    Effect::new(move |_| {
        if settings_open.get() {
            load_account_notify();
        }
    });
    let toggle_account_notify = move |id: i64| {
        let next = !notify_accounts
            .get_untracked()
            .get(&id)
            .copied()
            .unwrap_or(true);
        notify_accounts.update(|m| {
            m.insert(id, next);
        });
        leptos::task::spawn_local(async move {
            let _ = api::set_bool_setting(&format!("notify_account_{id}"), next).await;
        });
    };
    let save_signature = move |text: String| {
        sig_text.set(text.clone());
        if let Some(aid) = account.get() {
            leptos::task::spawn_local(async move {
                let _ = api::set_signature(aid, &text).await;
            });
        }
    };
    // Whether the selected folder is the Trash — enables Empty Trash and turns Delete into a
    // permanent delete (the message is already in Trash).
    let in_trash = move || {
        folders
            .get()
            .iter()
            .find(|f| selected_folder.get() == Some(f.id))
            .is_some_and(|f| is_trash_folder(&f.name, f.role.as_deref()))
    };

    // Empty the current Trash folder — permanent, confirmed. Re-lists (now empty) on success.
    let do_empty_trash = move || {
        trash_ask.set(None);
        let (Some(aid), Some(fid)) = (account.get(), selected_folder.get()) else {
            return;
        };
        open.set(None);
        leptos::task::spawn_local(async move {
            match api::empty_trash(aid).await {
                Ok(()) => toast.set(Some("Trash emptied".into())),
                Err(e) => error.set(Some(e)),
            }
            if let Ok(m) = api::list_messages(fid, PAGE).await {
                messages.set(m);
            }
        });
    };

    // Permanently delete one message that's already in Trash — confirmed, no undo. Optimistic remove.
    let do_delete_forever = move |id: i64| {
        trash_ask.set(None);
        let snapshot = messages.get_untracked();
        messages.update(|l| l.retain(|m| m.id != id));
        open.set(None);
        leptos::task::spawn_local(async move {
            match api::delete_forever(id).await {
                Ok(()) => toast.set(Some("Deleted".into())),
                Err(e) => {
                    messages.set(snapshot);
                    error.set(Some(e));
                }
            }
        });
    };

    let do_remove = move || {
        let Some((aid, _)) = confirm.get() else {
            return;
        };
        confirm.set(None);
        settings_open.set(false);
        leptos::task::spawn_local(async move {
            match api::remove_account(aid).await {
                Ok(_) => {
                    unified.set(false); // fall back to a concrete account, not a stale merged view
                    if let Ok(list) = api::list_accounts().await {
                        let first = list.first().map(|a| a.id);
                        accounts.set(list);
                        account.set(first);
                        if let Some(fid) = first {
                            load_folders(fid, folders, selected_folder, messages, error, request)
                                .await;
                        } else {
                            folders.set(Vec::new());
                            messages.set(Vec::new());
                            open.set(None);
                        }
                    }
                    commit_pending(); // resolve any queued move before this confirmation toast
                    toast.set(Some("Account removed".into()));
                }
                Err(e) => error.set(Some(e)),
            }
        });
    };

    // compose
    let reset_recipient_inputs = move || {
        to_input.set(String::new());
        cc_input.set(String::new());
        to_suggest.set(Vec::new());
        cc_suggest.set(Vec::new());
        attach_paths.set(Vec::new());
        md_on.set(false); // each new draft starts as plain text
        current_draft_id.set(None); // a fresh compose isn't tied to a saved draft yet
        resumed_server.set(None); // …nor to one of the provider's
    };
    // Open the native file picker and add the chosen files to the draft.
    let attach_files = move || {
        leptos::task::spawn_local(async move {
            match api::pick_files().await {
                Ok(paths) => attach_paths.update(|list| {
                    for p in paths {
                        if !list.contains(&p) {
                            list.push(p);
                        }
                    }
                }),
                Err(e) => error.set(Some(e)),
            }
        });
    };
    let compose_new = move || {
        reset_recipient_inputs();
        compose.set(Some(ComposeDraft::default()));
    };
    let compose_from_open = move |kind: &'static str| {
        let Some(id) = open.get().map(|b| b.id) else {
            return;
        };
        leptos::task::spawn_local(async move {
            match api::compose_draft(id, kind).await {
                Ok(d) => {
                    reset_recipient_inputs();
                    compose.set(Some(d));
                }
                Err(e) => error.set(Some(e)),
            }
        });
    };
    // Discard the draft (danger action in the compose footer / Esc).
    let discard_compose = move || {
        reset_recipient_inputs();
        compose.set(None);
    };
    let send_compose = move || {
        let (Some(aid), Some(mut d)) = (account.get(), compose.get()) else {
            return;
        };
        if sending.get() {
            return;
        }
        // Fold any recipient text still in the input boxes (typed but not turned into a chip),
        // de-duplicating so a repeated address never rides into the envelope twice.
        d.to = merge_addrs(&d.to, &to_input.get_untracked());
        d.cc = merge_addrs(&d.cc, &cc_input.get_untracked());
        if d.to.trim().is_empty() && d.cc.trim().is_empty() {
            error.set(Some("Add at least one recipient.".to_owned()));
            return;
        }
        sending.set(true);
        let atts = attach_paths.get_untracked();
        let markdown = md_on.get_untracked();
        let draft_id = current_draft_id.get_untracked(); // if resumed, run_send deletes it after send
        let replaced = resumed_server.get_untracked(); // a draft of the provider's, now sent
        leptos::task::spawn_local(async move {
            match api::send_message(
                aid,
                d.to,
                d.cc,
                d.subject,
                d.body,
                d.in_reply_to,
                d.references,
                atts,
                markdown,
                draft_id,
            )
            .await
            {
                Ok(queued) => {
                    compose.set(None);
                    reset_recipient_inputs();
                    // The sent draft (if any) is gone server-side; drop its row from the open list.
                    if let Some(id) = draft_id {
                        // …a LOCAL draft: a row's id only identifies it together with its origin (a
                        // draft id and a message id can be the same number).
                        drafts.update(|l| l.retain(|d| d.on_server || d.id != id));
                    }
                    // A draft of the provider's that we just sent is no longer a draft. The engine
                    // expunges the provider copy itself — on immediate send, and (via the outbox, which
                    // now carries the draft reference) when a queued send finally goes out. The UI just
                    // drops the row from the open Drafts list.
                    if let Some(mid) = replaced {
                        drafts.update(|l| l.retain(|d| !(d.on_server && d.id == mid)));
                    }
                    commit_pending(); // resolve any queued move before this confirmation toast
                                      // Offline: the message is safe in the outbox and goes out on the next sync.
                    toast.set(Some(if queued {
                        "Queued — will send when you're back online".into()
                    } else {
                        "Sent".into()
                    }));
                    refresh_outbox();
                }
                Err(e) => error.set(Some(e)),
            }
            sending.set(false);
        });
    };

    // Load the account's saved drafts into the list pane (the "Drafts" rail entry).
    let open_drafts = move || {
        unified.set(false);
        drafts_open.set(true);
        outbox_open.set(false);
        selected.set(HashSet::new()); // leaving the message list drops any bulk selection
        selected_folder.set(None);
        open.set(None);
        search_open.set(false);
        query.set(String::new());
        acct_menu.set(false);
        let Some(aid) = account.get() else {
            drafts.set(Vec::new());
            return;
        };
        drafts_loading.set(true);
        leptos::task::spawn_local(async move {
            // From the store first — instant, and it works offline (P1). Guarded like every other
            // re-list: the store call is a spawn_blocking behind a Mutex, so two of these can land out
            // of order, and painting account A's drafts into account B's open pane would let the user
            // resume a draft that isn't theirs to send from here.
            let mine = || drafts_open.get_untracked() && account.get_untracked() == Some(aid);
            match api::list_drafts(aid).await {
                Ok(list) if mine() => drafts.set(list),
                Ok(_) => {
                    drafts_loading.set(false); // the pane moved on; don't leave it saying "checking…"
                    return;
                }
                Err(e) => error.set(Some(e)),
            }
            // Then catch up with the provider's Drafts folder, which nothing else syncs (the
            // scheduler sweeps INBOX), and re-list if it brought anything. Quiet on failure: an
            // offline drafts list is the local one, which is exactly what the user expects to see.
            let synced = api::refresh_drafts(aid).await.unwrap_or(false);
            // …and again after the sync: the user may well have moved on while it ran.
            if synced && mine() {
                if let Ok(list) = api::list_drafts(aid).await {
                    drafts.set(list);
                }
            }
            drafts_loading.set(false);
        });
    };

    // Save the current compose form as a draft (new or updating the one being edited), then close.
    let save_current_draft = move || {
        let (Some(aid), Some(mut d)) = (account.get(), compose.get()) else {
            return;
        };
        // Fold any half-typed recipient text into the draft so nothing typed is lost on save.
        d.to = merge_addrs(&d.to, &to_input.get_untracked());
        d.cc = merge_addrs(&d.cc, &cc_input.get_untracked());
        let existing = current_draft_id.get_untracked();
        // A completely empty compose (nothing typed) isn't worth a draft row — just close it. A draft
        // being *edited* still saves, so clearing its fields and saving records the (now empty) update.
        let blank = d.to.trim().is_empty()
            && d.cc.trim().is_empty()
            && d.subject.trim().is_empty()
            && d.body.trim().is_empty();
        if blank && existing.is_none() {
            compose.set(None);
            reset_recipient_inputs();
            return;
        }
        // Capture the attachment paths and the server draft being replaced BEFORE the reset clears
        // them, so they're stored with the draft (and the original is taken off the server).
        let atts = attach_paths.get_untracked();
        let replaced = resumed_server.get_untracked();
        compose.set(None);
        reset_recipient_inputs();
        let showing_drafts = drafts_open.get_untracked();
        leptos::task::spawn_local(async move {
            match api::save_draft(aid, existing, d, atts).await {
                Ok(_) => {
                    toast.set(Some("Draft saved".into()));
                    // This draft *was* the provider's: it's ours now, so take the original off the
                    // server — only now that the local save has succeeded, so a failure can never
                    // lose it. (Left there, it would be back in the list as a second row.)
                    if let Some(mid) = replaced {
                        if api::delete_forever(mid).await.is_err() {
                            // Say what actually happened: the draft IS saved here, and the copy is
                            // still on the provider — so Drafts will show both until it can be removed.
                            error.set(Some(
                                "Draft saved here, but the copy on your provider couldn't be removed \
                                 — it will still show in Drafts."
                                    .to_owned(),
                            ));
                        }
                    }
                    // Keep the drafts list current if it's the pane on screen.
                    if showing_drafts {
                        if let Ok(list) = api::list_drafts(aid).await {
                            drafts.set(list);
                        }
                    }
                }
                Err(e) => error.set(Some(e)),
            }
        });
    };

    // Reopen a saved draft in the composer, tied to its id so edits update the same row.
    let resume_draft = move |id: i64| {
        leptos::task::spawn_local(async move {
            match api::load_draft(id).await {
                Ok(Some(ResumedDraft { draft, attachments })) => {
                    reset_recipient_inputs(); // clears attach_paths + current_draft_id first
                    compose.set(Some(draft));
                    attach_paths.set(attachments);
                    current_draft_id.set(Some(id));
                }
                Ok(None) => {
                    // Already gone (e.g. sent elsewhere) — drop the stale row.
                    drafts.update(|l| l.retain(|d| d.on_server || d.id != id));
                }
                Err(e) => error.set(Some(e)),
            }
        });
    };

    // Continue a draft that's in the provider's Drafts folder. Its text is already here; its
    // attachments are fetched now (that's the one part that needs the network). The original stays on
    // the server until the draft is actually saved or sent — abandon it and nothing has changed there.
    let resume_server_draft = move |id: i64| {
        formatted_ask.set(None);
        if draft_busy.get_untracked() {
            return; // a second click would fetch it twice and leave the loser's files behind
        }
        draft_busy.set(true);
        leptos::task::spawn_local(async move {
            match api::resume_server_draft(id).await {
                Ok(ResumedDraft { draft, attachments }) => {
                    reset_recipient_inputs(); // clears attach_paths + both draft ids first
                    compose.set(Some(draft));
                    attach_paths.set(attachments);
                    resumed_server.set(Some(id));
                }
                Err(e) => error.set(Some(e)),
            }
            draft_busy.set(false);
        });
    };

    // Open a draft row — from this device or from the provider. A formatted one asks first: the
    // composer writes plain text, and saving *replaces* the original (unlike a reply, where the
    // formatted message survives untouched).
    let open_draft_row = move |d: &DraftSummary| {
        let id = d.id;
        if !d.on_server {
            resume_draft(id);
        } else if d.formatted {
            formatted_ask.set(Some(id));
        } else {
            resume_server_draft(id);
        }
    };

    // Delete a saved draft from the list (its own row affordance). One on the provider is expunged
    // there — it isn't ours to keep a copy of.
    let remove_draft = move |id: i64, on_server: bool| {
        // The provider's copy may be the only one there is, and this expunges it — so ask, the way
        // every other irreversible action in the app does.
        if on_server {
            draft_del.set(Some(id));
            return;
        }
        let snapshot = drafts.get_untracked();
        drafts.update(|l| l.retain(|d| d.on_server || d.id != id));
        leptos::task::spawn_local(async move {
            if let Err(e) = api::delete_draft(id).await {
                drafts.set(snapshot); // it's still there — don't leave the list lying about it
                error.set(Some(e));
            }
        });
    };

    // …and the confirmed one: expunge a draft from the provider.
    let do_remove_server_draft = move || {
        let Some(id) = draft_del.get_untracked() else {
            return;
        };
        draft_del.set(None);
        let snapshot = drafts.get_untracked();
        drafts.update(|l| l.retain(|d| !(d.on_server && d.id == id)));
        leptos::task::spawn_local(async move {
            match api::delete_forever(id).await {
                Ok(()) => toast.set(Some("Deleted from your provider".into())),
                Err(_) => {
                    // Offline is the ordinary case here, and the draft is still on the provider — so
                    // put the row back rather than let the list claim it's gone.
                    drafts.set(snapshot);
                    error.set(Some(
                        "Couldn't delete that draft from your provider. Check your connection."
                            .to_owned(),
                    ));
                }
            }
        });
    };

    // Save one of the open message's attachments to disk (READ-8) — fetched on demand from the server.
    let save_att = move |message_id: i64, index: usize| {
        leptos::task::spawn_local(async move {
            match api::save_attachment(message_id, index).await {
                Ok(true) => toast.set(Some("Saved to disk".into())),
                Ok(false) => {} // cancelled
                Err(e) => error.set(Some(e)),
            }
        });
    };

    // Save the open message to disk as a .eml file (READ-10). No-op if nothing is open.
    let save_open_eml = move || {
        let Some(id) = open.get_untracked().map(|b| b.id) else {
            return;
        };
        leptos::task::spawn_local(async move {
            match api::save_eml(id).await {
                Ok(true) => toast.set(Some("Saved to disk".into())),
                Ok(false) => {} // the user cancelled the save dialog
                Err(e) => error.set(Some(e)),
            }
        });
    };

    // Open a .eml file from disk into the local Saved folder, then switch there and open it.
    let open_mail_file = move || {
        acct_menu.set(false);
        let Some(aid) = account.get() else {
            error.set(Some(
                "Add an account first, then open a mail file.".to_owned(),
            ));
            return;
        };
        leptos::task::spawn_local(async move {
            match api::open_eml_file(aid).await {
                Ok(Some(id)) => {
                    // The import created a local "Saved" folder — reload the rail so it shows.
                    if let Ok(fs) = api::list_folders(aid).await {
                        folders.set(fs);
                    }
                    if let Some(f) = folders
                        .get_untracked()
                        .into_iter()
                        .find(|f| f.name.eq_ignore_ascii_case("Saved"))
                    {
                        choose_folder(f.id);
                    }
                    choose_message(id);
                }
                Ok(None) => {} // cancelled
                Err(e) => error.set(Some(e)),
            }
        });
    };

    // Submit the New-folder / Rename-folder dialog.
    let submit_folder = move || {
        let (Some(form), Some(aid)) = (folder_form.get(), account.get()) else {
            return;
        };
        let name = form.name.trim().to_owned();
        if name.is_empty() {
            return;
        }
        folder_form.set(None);
        let renaming = form.rename_from.is_some();
        leptos::task::spawn_local(async move {
            let res = match form.rename_from {
                Some(from) => api::rename_folder(aid, from, name).await,
                None => api::create_folder(aid, name).await.map(|_| ()),
            };
            match res {
                Ok(()) => {
                    if let Ok(fs) = api::list_folders(aid).await {
                        folders.set(fs);
                    }
                    toast.set(Some(
                        if renaming {
                            "Folder renamed"
                        } else {
                            "Folder created"
                        }
                        .into(),
                    ));
                }
                Err(e) => error.set(Some(e)),
            }
        });
    };

    // Delete the folder in the confirm dialog (server + local, with its messages).
    let do_delete_folder = move || {
        let (Some((fid, name)), Some(aid)) = (folder_del.get(), account.get()) else {
            return;
        };
        folder_del.set(None);
        let was_selected = selected_folder.get_untracked() == Some(fid);
        leptos::task::spawn_local(async move {
            match api::delete_folder(aid, fid, name).await {
                Ok(()) => {
                    if let Ok(fs) = api::list_folders(aid).await {
                        folders.set(fs);
                    }
                    // If the open folder was the one deleted, fall back to the first (Inbox).
                    if was_selected {
                        if let Some(f) = folders.get_untracked().into_iter().next() {
                            choose_folder(f.id);
                        }
                    }
                    toast.set(Some("Folder deleted".into()));
                }
                Err(e) => error.set(Some(e)),
            }
        });
    };

    let do_refresh = move || {
        // Merged view: sync every account's INBOX, then re-list the combined inbox.
        if unified.get() {
            if refreshing.get() {
                return;
            }
            refreshing.set(true);
            let epoch = request.get_untracked() + 1;
            request.set(epoch);
            let accs = accounts.get_untracked();
            leptos::task::spawn_local(async move {
                for a in &accs {
                    let _ = api::refresh(a.id, "INBOX").await;
                }
                if let Ok(m) = api::list_all_messages(PAGE).await {
                    if request.get_untracked() == epoch {
                        messages.set(m);
                    }
                }
                refreshing.set(false);
            });
            return;
        }
        let (Some(aid), Some(fid)) = (account.get(), selected_folder.get()) else {
            return;
        };
        let folder_name = folders
            .get()
            .into_iter()
            .find(|f| f.id == fid)
            .map(|f| f.name)
            .unwrap_or_default();
        if refreshing.get() || catchup.get().is_some() || folder_name.is_empty() {
            return;
        }
        refreshing.set(true);
        catchup.set(Some(0));
        let epoch = request.get_untracked() + 1;
        request.set(epoch);
        let q = query.get_untracked();
        leptos::task::spawn_local(async move {
            match api::refresh(aid, &folder_name).await {
                Ok(()) => {
                    let result = if q.trim().is_empty() {
                        api::list_messages(fid, PAGE).await
                    } else {
                        api::search(aid, &q).await
                    };
                    if let Ok(m) = result {
                        if request.get_untracked() == epoch {
                            messages.set(m);
                        }
                    }
                    // refresh the rail's unread counts too — but only if this account is still the
                    // selected one (the user may have switched away while the refresh was in flight)
                    if let Ok(fs) = api::list_folders(aid).await {
                        if account.get_untracked() == Some(aid) {
                            folders.set(fs);
                        }
                    }
                }
                Err(e) => {
                    catchup.set(None);
                    error.set(Some(e));
                }
            }
            refreshing.set(false);
        });
    };

    // Dismiss the toast after its window, cancelling any previous timer so a fresh toast that
    // replaces an older one still gets its full lifetime. If a move is pending, the window elapsing
    // is what *commits* it to the server (so the whole window is the Undo grace period).
    Effect::new(move |_| {
        if toast.get().is_some() {
            #[cfg(target_arch = "wasm32")]
            {
                if let Some(w) = web_sys::window() {
                    if let Some(prev) = toast_timer.get_untracked() {
                        w.clear_timeout_with_handle(prev);
                    }
                    let cb = wasm_bindgen::closure::Closure::once_into_js(move || {
                        if pending.get_untracked().is_some() {
                            commit_pending();
                        } else {
                            toast.set(None);
                        }
                    });
                    if let Ok(id) = w.set_timeout_with_callback_and_timeout_and_arguments_0(
                        cb.as_ref().unchecked_ref(),
                        5000,
                    ) {
                        toast_timer.set(Some(id));
                    }
                }
            }
        }
    });

    // Dev-only screenshot seam (the commands return nothing in a release build): `GELEIT_OPEN=<id>`
    // opens a message on launch (`GELEIT_IMAGES=1` loads its remote content), `GELEIT_COMPOSE=<kind>`
    // opens the composer. Used to drive the UI into a state for a screenshot without click injection.
    // Waits for boot to finish (`loaded`) so preferences like mark-as-read are already in effect.
    Effect::new(move |ran: Option<bool>| {
        let ran = ran.unwrap_or(false);
        if ran || !loaded.get() {
            return ran;
        }
        leptos::task::spawn_local(async move {
            if api::dev_unified().await.unwrap_or(false) {
                choose_unified();
            }
            let opened = api::dev_open_message().await.ok().flatten();
            if let Some(id) = opened {
                if api::dev_load_images().await.unwrap_or(false) {
                    load_images.set(true);
                }
                choose_message(id);
            }
            if let Ok(Some(kind)) = api::dev_compose().await {
                if kind == "new" {
                    compose_new();
                    // Optionally pre-fill the To input to surface the autocomplete dropdown.
                    if let Ok(Some(prefix)) = api::dev_compose_to().await {
                        to_input.set(prefix);
                    }
                } else if let Some(id) = opened {
                    // Build the reply/forward draft straight from the opened id — `open` may not be
                    // set yet, so don't route through `compose_from_open` (it reads `open`).
                    if let Ok(d) = api::compose_draft(id, &kind).await {
                        compose.set(Some(d));
                    }
                }
            }
            if api::dev_setup().await.unwrap_or(false) {
                open_wizard();
            }
            if let Ok(Some(tab)) = api::dev_settings().await {
                open_settings();
                if tab != "1" {
                    settings_tab.set(tab); // e.g. GELEIT_SETTINGS=privacy
                }
            }
            if let Ok(Some(q)) = api::dev_search().await {
                search_open.set(true);
                run_search(q);
            }
            if let Ok(Some(kind)) = api::dev_trash().await {
                trash_ask.set(Some(if kind == "delete" {
                    TrashAsk::DeleteOne(-1)
                } else {
                    TrashAsk::Empty
                }));
            }
            if api::dev_drafts().await.unwrap_or(false) {
                open_drafts();
            }
            if api::dev_resume().await.unwrap_or(false) {
                if let Some(aid) = account.get_untracked() {
                    if let Ok(list) = api::list_drafts(aid).await {
                        if let Some(first) = list.first() {
                            // Through the same door a click uses: the newest draft may well be one of
                            // the provider's, whose id is a MESSAGE id and must not be resumed as a
                            // local draft.
                            open_draft_row(first);
                        }
                    }
                }
            }
            if let Ok(Some(ids)) = api::dev_select().await {
                let set: HashSet<i64> = ids
                    .split(',')
                    .filter_map(|s| s.trim().parse().ok())
                    .collect();
                selected.set(set);
            }
            if let Ok(Some(kind)) = api::dev_folder().await {
                if kind == "new" {
                    folder_form.set(Some(FolderForm {
                        rename_from: None,
                        name: String::new(),
                    }));
                } else if kind == "menu" {
                    if let Some(f) = folders
                        .get_untracked()
                        .into_iter()
                        .find(|f| !is_protected_folder(&f.name, f.role.as_deref()))
                    {
                        folder_menu.set(Some(f.id));
                    }
                }
            }
        });
        true
    });

    // keyboard shortcuts on the document (c compose, / search, e archive, r reply, f forward, Esc close)
    #[cfg(target_arch = "wasm32")]
    {
        let handler = wasm_bindgen::closure::Closure::<dyn Fn(web_sys::KeyboardEvent)>::new(
            move |e: web_sys::KeyboardEvent| {
                let tag = e
                    .target()
                    .and_then(|t| t.dyn_ref::<web_sys::Element>().map(|el| el.tag_name()))
                    .unwrap_or_default();
                let typing = matches!(tag.as_str(), "INPUT" | "TEXTAREA");
                let key = e.key();
                if key == "Escape" {
                    // An open recipient-autocomplete dropdown swallows Escape (close it, keep the
                    // draft) — the input's own handler can't win this race because the global
                    // listener sits on `document` and fires before Leptos's window-delegated one.
                    if !to_suggest.get_untracked().is_empty()
                        || !cc_suggest.get_untracked().is_empty()
                    {
                        to_suggest.set(Vec::new());
                        cc_suggest.set(Vec::new());
                    } else if compose.get_untracked().is_some() {
                        discard_compose();
                    } else if wiz.get_untracked().is_some() {
                        wiz.set(None);
                    } else if settings_open.get_untracked() {
                        settings_open.set(false);
                    } else if folder_form.get_untracked().is_some() {
                        folder_form.set(None);
                    } else if folder_del.get_untracked().is_some() {
                        folder_del.set(None);
                    } else if formatted_ask.get_untracked().is_some() {
                        formatted_ask.set(None);
                    } else if draft_del.get_untracked().is_some() {
                        draft_del.set(None);
                    } else if confirm.get_untracked().is_some() {
                        confirm.set(None);
                    } else if move_menu.get_untracked()
                        || acct_menu.get_untracked()
                        || folder_menu.get_untracked().is_some()
                    {
                        move_menu.set(false);
                        acct_menu.set(false);
                        folder_menu.set(None);
                    } else if search_open.get_untracked() {
                        // close the search box and return to the current folder / merged view
                        search_open.set(false);
                        run_search(String::new());
                    }
                    return;
                }
                if typing
                    || compose.get_untracked().is_some()
                    || wiz.get_untracked().is_some()
                    || settings_open.get_untracked()
                    || e.meta_key()
                    || e.ctrl_key()
                {
                    return;
                }
                match key.as_str() {
                    "c" => compose_new(),
                    "/" => {
                        search_open.set(true);
                        e.prevent_default();
                    }
                    "j" | "ArrowDown" => {
                        nav(1);
                        e.prevent_default();
                    }
                    "k" | "ArrowUp" => {
                        nav(-1);
                        e.prevent_default();
                    }
                    "z" if pending.get_untracked().is_some() => undo_pending(),
                    "e" if open.get_untracked().is_some() => move_open("archive", "Archived"),
                    "#" if open.get_untracked().is_some() => {
                        if in_trash() {
                            if let Some(b) = open.get_untracked() {
                                trash_ask.set(Some(TrashAsk::DeleteOne(b.id)));
                            }
                        } else {
                            move_open("trash", "Deleted");
                        }
                    }
                    "r" if open.get_untracked().is_some() => compose_from_open("reply"),
                    "f" if open.get_untracked().is_some() => compose_from_open("forward"),
                    _ => {}
                }
            },
        );
        if let Some(d) = web_sys::window().and_then(|w| w.document()) {
            let _ = d.add_event_listener_with_callback("keydown", handler.as_ref().unchecked_ref());
        }
        handler.forget();
    }

    // ---- derived views ----
    let account_initial = move || {
        accounts
            .get()
            .iter()
            .find(|a| Some(a.id) == account.get())
            .and_then(|a| a.email.chars().next())
            .map(|c| c.to_ascii_uppercase().to_string())
            .unwrap_or_else(|| "A".into())
    };
    let current_email = move || {
        accounts
            .get()
            .iter()
            .find(|a| Some(a.id) == account.get())
            .map(|a| a.email.clone())
            .unwrap_or_default()
    };
    let current_color = move || {
        accounts
            .get()
            .iter()
            .position(|a| Some(a.id) == account.get())
            .map(account_color)
            .unwrap_or("transparent")
    };
    // For merged-view rows, look up a specific account's email / dot colour by id.
    let email_of = move |id: i64| {
        accounts
            .get()
            .iter()
            .find(|a| a.id == id)
            .map(|a| a.email.clone())
            .unwrap_or_default()
    };
    let color_of = move |id: i64| {
        accounts
            .get()
            .iter()
            .position(|a| a.id == id)
            .map(account_color)
            .unwrap_or("transparent")
    };

    // Accounts paired with their position (for the indicator colour). A `Copy` closure so it can feed
    // more than one `<For>`; the turbofish would otherwise trip the view macro's tag parser.
    let accounts_indexed = move || {
        accounts
            .get()
            .into_iter()
            .enumerate()
            .collect::<Vec<(usize, Account)>>()
    };

    // Flatten the list into day-labelled rows. Messages are aggregated into the three fixed buckets
    // (so each label appears at most once even when the source isn't date-ordered — search returns
    // rank order, not date order), then emitted as a flat header/message stream for a single keyed
    // `<For>`. Keys are unique and stable: `h:<label>` for a header, `m:<id>` for a message.
    let rows = move || {
        if drafts_open.get() || outbox_open.get() {
            return Vec::new(); // the drafts / outbox panes render their own rows
        }
        let hidden = pending.get().map(|p| p.ids).unwrap_or_default();
        let mut buckets: [(&'static str, Vec<Message>); 3] = [
            ("Today", Vec::new()),
            ("Yesterday", Vec::new()),
            ("Earlier", Vec::new()),
        ];
        for m in messages.get() {
            if hidden.contains(&m.id) {
                continue; // hidden during its Undo window
            }
            let slot = match day_bucket(m.date) {
                "Today" => 0,
                "Yesterday" => 1,
                _ => 2,
            };
            buckets[slot].1.push(m);
        }
        let mut out: Vec<ListRow> = Vec::new();
        for (label, items) in buckets {
            if items.is_empty() {
                continue;
            }
            out.push(ListRow::Header(label));
            out.extend(items.into_iter().map(ListRow::Msg));
        }
        out
    };

    view! {
        <div class="app">
            // ============ RAIL ============
            <nav class="rail" class:collapsed=move || rail_collapsed.get()>
                <Show
                    when=move || !rail_collapsed.get()
                    fallback=move || view! {
                        <div class="avatar" title="Accounts" on:click=move |_| acct_menu.update(|o| *o = !*o)>{account_initial}</div>
                        <button class="compose-btn" title="Compose" on:click=move |_| compose_new()>{icon(icons::PLUS)}</button>
                        <For each=move || folders.get() key=|f| f.id let:f>
                            {
                                let (id, name, role) = (f.id, f.name.clone(), f.role.clone());
                                view! {
                                    <button class="folder" class:active=move || selected_folder.get() == Some(id)
                                        title=name.clone() on:click=move |_| choose_folder(id)>
                                        <span class="guide"></span>
                                        <span class="fic">{icon(icons::folder_icon(&name, role.as_deref()))}</span>
                                    </button>
                                }
                            }
                        </For>
                        <div class="rail-fill"></div>
                        <button class="rail-tool" title="Settings" on:click=move |_| open_settings()>{icon(icons::SETTINGS)}</button>
                        <button class="rail-btn" title="Expand" on:click=move |_| { rail_collapsed.set(false); }>{icon(icons::EXPAND)}</button>
                    }
                >
                    <div class="acct-head">
                        <div class="acct-switch" on:click=move |_| acct_menu.update(|o| *o = !*o)>
                            <div class="avatar">{move || if unified.get() { "\u{2211}".to_string() } else { account_initial() }}</div>
                            <div style="min-width:0;flex:1">
                                <div class="acct-name">{move || if unified.get() { "All inboxes".to_string() } else { current_email() }}</div>
                                <div class="acct-sub">
                                    {move || {
                                        let n = accounts.get().len();
                                        if n > 1 { format!("{n} accounts") } else { "switch account".into() }
                                    }}
                                    {icon(icons::CHEVRON_DOWN)}
                                </div>
                            </div>
                        </div>
                        <button class="rail-btn" title="Collapse" on:click=move |_| { rail_collapsed.set(true); acct_menu.set(false); }>{icon(icons::COLLAPSE)}</button>
                        <Show when=move || acct_menu.get()>
                            <div class="menu" style="left:4px;top:46px;width:200px">
                                <Show when=move || { accounts.get().len() > 1 }>
                                    <div class="menu-item" class:sel=move || unified.get()
                                        on:click=move |_| choose_unified()>
                                        {icon(icons::INBOX)} "All inboxes"
                                    </div>
                                    <div class="menu-sep"></div>
                                </Show>
                                <For each=accounts_indexed key=|(_, a)| a.id let:pair>
                                    {
                                        let (i, a) = pair;
                                        let (id, email) = (a.id, a.email.clone());
                                        view! {
                                            <div class="menu-item" class:sel=move || !unified.get() && account.get() == Some(id)
                                                on:click=move |_| switch_account(id)>
                                                <span class="dot" style=format!("background:{}", account_color(i))></span>
                                                {email}
                                            </div>
                                        }
                                    }
                                </For>
                                <div class="menu-sep"></div>
                                <div class="menu-item add" on:click=move |_| open_wizard()>
                                    {icon(icons::PLUS)} "Add account"
                                </div>
                            </div>
                        </Show>
                    </div>
                    <button class="compose-btn" on:click=move |_| compose_new()>{icon(icons::PLUS)} "Compose"</button>
                    // Outbox indicator (SEND-10): only shown when something is waiting or was rejected,
                    // so a quiet outbox is invisible. Failed sends read as a warning, not a count.
                    <Show when=move || { outbox.get().0 > 0 || outbox.get().1 > 0 }>
                        <div class="outbox-note" class:warn=move || { outbox.get().1 > 0 } class:active=move || outbox_open.get()
                            role="button" tabindex="0" title="Open the outbox"
                            on:click=move |_| open_outbox()
                            on:keydown=move |e| { if e.key() == "Enter" || e.key() == " " { e.prevent_default(); open_outbox(); } }>
                            {move || {
                                let (queued, failed) = outbox.get();
                                if failed > 0 {
                                    format!("{failed} couldn't send{}", if queued > 0 { format!(", {queued} waiting") } else { String::new() })
                                } else if queued == 1 {
                                    "1 message waiting to send".to_owned()
                                } else {
                                    format!("{queued} messages waiting to send")
                                }
                            }}
                        </div>
                    </Show>
                    // In the merged view, folders are per-account and don't apply — show a single
                    // "All inboxes" marker instead of a folder list.
                    <Show
                        when=move || !unified.get()
                        fallback=|| view! {
                            <button class="folder active">
                                <span class="guide"></span>
                                <span class="fic">{icon(icons::INBOX)}</span>
                                "All inboxes"
                            </button>
                        }
                    >
                        <For each=move || folders.get() key=|f| f.id let:f>
                            {
                                let (id, name, unread) = (f.id, f.name.clone(), f.unread);
                                let label = name.clone();
                                let role = f.role.clone();
                                let protected = is_protected_folder(&name, role.as_deref());
                                let rename_name = name.clone();
                                let del_name = name.clone();
                                view! {
                                    <div class="folder" role="button" tabindex="0"
                                        class:active=move || selected_folder.get() == Some(id) && !drafts_open.get()
                                        on:click=move |_| choose_folder(id)
                                        on:keydown=move |e| { if e.key() == "Enter" || e.key() == " " { e.prevent_default(); choose_folder(id); } }>
                                        <span class="guide"></span>
                                        <span class="fic">{icon(icons::folder_icon(&name, role.as_deref()))}</span>
                                        {label}
                                        <span class="fcount">{move || if unread > 0 { unread.to_string() } else { String::new() }}</span>
                                        <Show when=move || !protected>
                                            <span class="folder-more" title="Folder options"
                                                on:click=move |e| { e.stop_propagation(); folder_menu.update(|m| *m = if *m == Some(id) { None } else { Some(id) }); }>
                                                {icon(icons::MORE)}
                                            </span>
                                        </Show>
                                        <Show when=move || folder_menu.get() == Some(id)>
                                            <div class="menu folder-menu">
                                                <div class="menu-item" on:click={
                                                    let n = rename_name.clone();
                                                    move |e: web_sys::MouseEvent| { e.stop_propagation(); folder_menu.set(None); folder_form.set(Some(FolderForm { rename_from: Some(n.clone()), name: n.clone() })); }
                                                }>{icon(icons::MOVE)} "Rename…"</div>
                                                <div class="menu-item danger" on:click={
                                                    let n = del_name.clone();
                                                    move |e: web_sys::MouseEvent| { e.stop_propagation(); folder_menu.set(None); folder_del.set(Some((id, n.clone()))); }
                                                }>{icon(icons::TRASH)} "Delete…"</div>
                                            </div>
                                        </Show>
                                    </div>
                                }
                            }
                        </For>
                        <button class="folder" class:active=move || drafts_open.get()
                            on:click=move |_| open_drafts()>
                            <span class="guide"></span>
                            <span class="fic">{icon(icons::DRAFTS)}</span>
                            "Drafts"
                        </button>
                        <button class="folder newfolder" on:click=move |_| folder_form.set(Some(FolderForm { rename_from: None, name: String::new() }))>
                            <span class="guide"></span>
                            <span class="fic">{icon(icons::PLUS)}</span>
                            "New folder"
                        </button>
                    </Show>
                    <div class="rail-fill"></div>
                    <button class="rail-tool" title="Open a .eml mail file from disk" on:click=move |_| open_mail_file()>
                        {icon(icons::OPENFILE)} "Open mail file…"
                    </button>
                    <button class="rail-tool" on:click=move |_| toggle_theme()>
                        {icon(icons::THEME)}
                        {move || if dark.get() { "Light theme" } else { "Dark theme" }}
                    </button>
                    <button class="rail-tool" on:click=move |_| open_settings()>{icon(icons::SETTINGS)} "Settings"</button>
                </Show>
            </nav>

            // ============ LIST ============
            <div class="list-col">
                <div class="list-head">
                    <div class="list-title">{move || if outbox_open.get() { "Outbox".to_string() } else if drafts_open.get() { "Drafts".to_string() } else if unified.get() { "All inboxes".to_string() } else { folders.get().into_iter().find(|f| selected_folder.get() == Some(f.id)).map(|f| f.name).unwrap_or_default() }}</div>
                    <div class="list-sub">{move || {
                        if outbox_open.get() {
                            let n = outbox_list.get().len();
                            return if n == 1 { "1 message".to_owned() } else if n > 0 { format!("{n} messages") } else { String::new() };
                        }
                        if drafts_open.get() {
                            let n = drafts.get().len();
                            // Not "saved" — some of these live on the provider, not here.
                            return match n {
                                0 => String::new(),
                                1 => "1 draft".to_owned(),
                                n => format!("{n} drafts"),
                            };
                        }
                        let n = messages.get().iter().filter(|m| is_unread(m.id, m.seen, read_now, marked_unread)).count();
                        if n > 0 { format!("{n} unread") } else { String::new() }
                    }}</div>
                    <div class="list-actions">
                        <Show when=in_trash>
                            <span class="icon-btn danger" title="Empty Trash" on:click=move |_| trash_ask.set(Some(TrashAsk::Empty))>{icon(icons::TRASH)}</span>
                        </Show>
                        <Show when=move || { !drafts_open.get() && !outbox_open.get() }>
                            <span class="icon-btn" class:on=move || search_open.get() title="Search" on:click=move |_| search_open.update(|o| *o = !*o)>{icon(icons::SEARCH)}</span>
                        </Show>
                        <span class="icon-btn" title="Refresh"
                            on:click=move |_| if drafts_open.get_untracked() { open_drafts() } else { do_refresh() }>
                            {icon(icons::REFRESH)}
                        </span>
                    </div>
                </div>
                <Show when=move || !drafts_open.get() && !selected.get().is_empty()>
                    <div class="bulk-bar">
                        <span class="rowcheck" title="Select all"
                            class:on=move || all_selected(&messages.get().iter().map(|m| m.id).collect::<Vec<_>>(), &selected.get())
                            on:click=move |_| {
                                let ids: Vec<i64> = messages.get_untracked().iter().map(|m| m.id).collect();
                                select_anchor.set(None);
                                if all_selected(&ids, &selected.get_untracked()) {
                                    selected.set(HashSet::new());
                                } else {
                                    selected.set(ids.into_iter().collect());
                                }
                            }>
                            {icon(icons::CHECK)}
                        </span>
                        <span class="bulk-count">{move || format!("{} selected", selected.get().len())}</span>
                        <span class="icon-btn" title="Archive" on:click=move |_| bulk_move("archive", "archived")>{icon(icons::ARCHIVE)}</span>
                        <span class="icon-btn danger" title="Delete" on:click=move |_| bulk_move("trash", "deleted")>{icon(icons::TRASH)}</span>
                        <span class="icon-btn" title="Mark read" on:click=move |_| bulk_mark_read()>{icon(icons::MAILOPEN)}</span>
                        <span class="icon-btn" title="Mark unread" on:click=move |_| bulk_mark_unread()>{icon(icons::UNREAD)}</span>
                        <span class="icon-btn" title="Clear selection" on:click=move |_| { selected.set(HashSet::new()); select_anchor.set(None); }>{icon(icons::CLOSE)}</span>
                    </div>
                </Show>
                <Show when=move || refreshing.get() || catchup.get().is_some() || drafts_loading.get() || draft_busy.get()>
                    <div style="padding:0 20px 8px;font-size:12px;color:var(--text-muted)">
                        {move || {
                            if draft_busy.get() {
                                // The one place a click here waits on the network: fetching the draft's
                                // files. Say so, rather than look frozen.
                                return "Opening the draft on your provider…".to_owned();
                            }
                            if drafts_loading.get() {
                                return "Checking your provider for drafts…".to_owned();
                            }
                            match catchup.get() {
                                Some(0) => "Checking for new mail…".to_owned(),
                                Some(n) => format!("Catching up… {n}"),
                                None => "Refreshing…".to_owned(),
                            }
                        }}
                    </div>
                </Show>
                <Show when=move || search_open.get()>
                    <div class="list-search">
                        <input placeholder="Search mail" prop:value=move || query.get()
                            on:input=move |e| run_search(event_target_value(&e))/>
                    </div>
                </Show>
                <div class="list-scroll" class:selecting=move || !selected.get().is_empty()>
                    <Show when=move || !drafts_open.get() && loaded.get() && messages.get().is_empty() && error.get().is_none()>
                        <div class="list-empty">
                            <Show when=move || account.get().is_none() fallback=|| view! { <div class="big">"✓"</div><div class="msg">"Nothing here."</div> }>
                                <div class="msg">"No account yet."</div>
                                <button class="btn-primary add" on:click=move |_| open_wizard()>"Add account"</button>
                            </Show>
                        </div>
                    </Show>
                    <Show when=move || drafts_open.get() && drafts.get().is_empty() && !drafts_loading.get()>
                        <div class="list-empty">
                            // Not "no saved drafts" — this list holds the provider's too, and saying
                            // "none" before we've looked there would be contradicted a second later.
                            <div class="msg">"No drafts."</div>
                        </div>
                    </Show>
                    <Show when=move || drafts_open.get()>
                        <For each=move || drafts.get() key=|d| (d.id, d.on_server, d.updated_at) let:d>
                            {
                                let (id, on_server) = (d.id, d.on_server);
                                let row = d.clone();
                                let subject = if d.subject.trim().is_empty() { "(no subject)".to_owned() } else { d.subject.clone() };
                                let to = if d.to.trim().is_empty() { "(no recipient)".to_owned() } else { d.to.clone() };
                                let snippet = elide(&d.snippet, 84);
                                let saved = if d.updated_at > 0 { local_date(Some(d.updated_at)) } else { String::new() };
                                view! {
                                    <div class="row draft" on:click=move |_| open_draft_row(&row)>
                                        <span class="guide"></span>
                                        <div class="row-top">
                                            <span class="sender">{to}</span>
                                            <span class="draftdel" title=move || if on_server { "Delete from your provider" } else { "Delete draft" }
                                                on:click=move |e| { e.stop_propagation(); remove_draft(id, on_server); }>
                                                {icon(icons::TRASH)}
                                            </span>
                                            <span class="time">{saved}</span>
                                        </div>
                                        <div class="subj">{subject}</div>
                                        <div class="prev">{snippet}</div>
                                        <Show when=move || on_server>
                                            // Where this draft lives, said plainly: it isn't on this device, and
                                            // deleting it here deletes it from the provider.
                                            <span class="draft-where">"On your provider"</span>
                                        </Show>
                                    </div>
                                }
                            }
                        </For>
                    </Show>
                    <Show when=move || { outbox_open.get() && outbox_list.get().is_empty() }>
                        <div class="list-empty"><div class="msg">"The outbox is empty."</div></div>
                    </Show>
                    <Show when=move || outbox_open.get()>
                        <For each=move || outbox_list.get() key=|o| (o.id, o.failed) let:o>
                            {
                                let id = o.id;
                                let to = if o.to.trim().is_empty() { "(no recipient)".to_owned() } else { o.to.clone() };
                                let subject = o.subject.clone();
                                let err = o.error.clone();
                                let failed = o.failed;
                                view! {
                                    <div class="row outbox-row">
                                        <span class="guide"></span>
                                        <div class="row-top">
                                            <span class="sender">{to}</span>
                                            <span class="time">{if failed { "couldn't send" } else { "waiting…" }}</span>
                                        </div>
                                        <div class="subj">{subject}</div>
                                        <Show when=move || failed>
                                            <div class="outbox-err">{err.clone()}</div>
                                        </Show>
                                        <div class="outbox-actions">
                                            <Show when=move || failed>
                                                <button class="btn-ghost small" on:click=move |_| retry_outbox_msg(id)>"Retry"</button>
                                            </Show>
                                            <button class="btn-ghost small danger" on:click=move |_| discard_outbox_msg(id)>"Discard"</button>
                                        </div>
                                    </div>
                                }
                            }
                        </For>
                    </Show>
                    <For
                        each=rows
                        key=|row| match row {
                            ListRow::Header(label) => format!("h:{label}"),
                            ListRow::Msg(m) => format!("m:{}", m.id),
                        }
                        let:row
                    >
                        {match row {
                            ListRow::Header(label) => {
                                Either::Left(view! { <div class="day-label">{label}</div> })
                            }
                            ListRow::Msg(m) => {
                                let id = m.id;
                                let was_seen = m.seen;
                                let date = local_date(m.date);
                                let snippet = elide(&m.snippet, 84);
                                let (from, subject) = (m.from.clone(), m.subject.clone());
                                let attach = m.has_attachments;
                                let convo = m.thread_count;
                                let macc = m.account; // owning account (merged view only)
                                Either::Right(view! {
                                    <div class="row"
                                        id=format!("row-{id}")
                                        class:unread=move || is_unread(id, was_seen, read_now, marked_unread)
                                        class:sel=move || open.get().is_some_and(|b| b.id == id)
                                        class:picked=move || selected.with(|s| s.contains(&id))
                                        on:click=move |_| choose_message(id)>
                                        <span class="guide"></span>
                                        <span class="rowcheck" class:on=move || selected.with(|s| s.contains(&id))
                                            title="Select"
                                            on:click=move |e| { e.stop_propagation(); select_click(id, e.shift_key()); }>
                                            <Show when=move || selected.with(|s| s.contains(&id))>{icon(icons::CHECK)}</Show>
                                        </span>
                                        <div class="row-top">
                                            <span class="udot"></span>
                                            <span class="sender">{from}</span>
                                            <Show when=move || attach>
                                                <span class="clip">{icon(icons::CLIP)}</span>
                                            </Show>
                                            <Show when=move || messages.with(|l| l.iter().find(|m| m.id == id).map(|m| m.flagged).unwrap_or(false))>
                                                <span class="rowstar">{icon(icons::STAR_FILLED)}</span>
                                            </Show>
                                            <span class="time">{date}</span>
                                        </div>
                                        <div class="subj">{subject}</div>
                                        <div class="prev">{snippet}</div>
                                        <div class="acct">
                                            <span class="dot" style=move || format!("background:{}", if unified.get() { color_of(macc) } else { current_color() })></span>
                                            {move || if unified.get() { email_of(macc) } else { current_email() }}
                                            <Show when=move || { convo > 1 }>
                                                <span>{move || format!("· conversation {convo}")}</span>
                                            </Show>
                                        </div>
                                    </div>
                                })
                            }
                        }}
                    </For>
                </div>
            </div>

            <div class="splitter"></div>

            // ============ READING ============
            <div class="read" class:has-mail=move || open.get().is_some_and(|b| b.is_html)>
                <div class="edge"></div>
                <Show
                    when=move || open.get().is_some()
                    fallback=|| view! { <div class="read-empty"><div class="msg">"Select a message to read"</div></div> }
                >
                    {move || open.get().map(|body| {
                        let id = body.id;
                        view! {
                            <>
                                <div class="read-inner">
                                    <div class="actions">
                                        <span class="act" class:starred=move || open_flagged.get() on:click=move |_| toggle_star()>
                                            {move || if open_flagged.get() { icon(icons::STAR_FILLED) } else { icon(icons::STAR) }}
                                            {move || if open_flagged.get() { "Starred" } else { "Star" }}
                                        </span>
                                        <span class="act" on:click=move |_| compose_from_open("reply")>{icon(icons::REPLY)} "Reply"</span>
                                        <span class="act" on:click=move |_| compose_from_open("reply_all")>{icon(icons::REPLY_ALL)} "Reply all"</span>
                                        <span class="act" on:click=move |_| compose_from_open("forward")>{icon(icons::FORWARD)} "Forward"</span>
                                        <span class="act" on:click=move |_| move_open("archive", "Archived")>{icon(icons::ARCHIVE)} "Archive"</span>
                                        <span class="act" on:click=move |_| move_menu.update(|o| *o = !*o)>{icon(icons::MOVE)} "Move"</span>
                                        <span class="act danger" on:click=move |_| { if in_trash() { trash_ask.set(Some(TrashAsk::DeleteOne(id))); } else { move_open("trash", "Deleted"); } }>{icon(icons::TRASH)} {move || if in_trash() { "Delete forever" } else { "Delete" }}</span>
                                        <span class="act" on:click=move |_| mark_unread()>{icon(icons::UNREAD)} "Unread"</span>
                                        <span class="act" title="Save this message as a .eml file" on:click=move |_| save_open_eml()>{icon(icons::DOWNLOAD)} "Save"</span>
                                        <Show when=move || move_menu.get()>
                                            <div class="menu" style="right:0;top:42px;width:180px">
                                                <For each=move || folders.get() key=|f| f.id let:f>
                                                    {
                                                        let name = f.name.clone();
                                                        let target = name.clone();
                                                        view! {
                                                            <div class="menu-item" on:click=move |_| move_open_folder(target.clone())>{name}</div>
                                                        }
                                                    }
                                                </For>
                                            </div>
                                        </Show>
                                    </div>
                                    <div class="read-from">
                                        <div class="read-avatar">{body.from.chars().next().map(|c| c.to_ascii_uppercase().to_string()).unwrap_or_default()}</div>
                                        <div class="read-sender">{body.from.clone()}</div>
                                        <div class="read-when">
                                            <span class="dot" style=move || format!("background:{}", current_color())></span>
                                            {local_date(body.date)}
                                        </div>
                                    </div>
                                    <h1>{body.subject.clone()}</h1>
                                </div>
                                <Show when=move || body.has_remote && !load_images.get()>
                                    <div class="cue">
                                        <span class="edge"></span>
                                        {icon(icons::WARN)}
                                        "Remote content blocked"
                                        <a class="load" on:click=move |_| load_images.set(true)>"Load images"</a>
                                    </div>
                                </Show>
                                {
                                    let atts = body.attachments.clone();
                                    (!atts.is_empty()).then(|| view! {
                                        <div class="attachments">
                                            {atts.into_iter().enumerate().map(|(i, a)| view! {
                                                <span class="att-chip">
                                                    {icon(icons::CLIP)}
                                                    <span class="att-name">{a.name}</span>
                                                    <span class="att-size">{a.size}</span>
                                                    <span class="att-save" title="Save to disk" on:click=move |_| save_att(id, i)>{icon(icons::DOWNLOAD)}</span>
                                                </span>
                                            }).collect_view()}
                                        </div>
                                    })
                                }
                                {if body.is_html {
                                    Either::Left(view! {
                                        <iframe class="mail" sandbox="allow-popups allow-popups-to-escape-sandbox"
                                            src=move || if load_images.get() { format!("mail://localhost/{id}?images=1") } else { format!("mail://localhost/{id}") }></iframe>
                                    })
                                } else {
                                    Either::Right(view! {
                                        <div class="read-body"><pre class="body-text">{body.plain.clone().unwrap_or_else(|| "This message has no text.".into())}</pre></div>
                                    })
                                }}
                            </>
                        }
                    })}
                </Show>
            </div>

            // ============ COMPOSE ============
            <Show when=move || compose.get().is_some()>
                <div class="scrim">
                    <div class="window compose">
                        <div class="win-head raised">
                            <span class="win-title">{move || {
                                let s = compose.get().map(|d| d.subject).unwrap_or_default().to_lowercase();
                                if s.starts_with("re:") { "Reply" } else if s.starts_with("fwd:") { "Forward" } else { "New message" }
                            }}</span>
                            <button class="close-btn" on:click=move |_| discard_compose()>{icon(icons::CLOSE)}</button>
                        </div>
                        <Show when=move || resumed_server.get().is_some()>
                            <div class="compose-note">
                                "From your provider's Drafts. Saving or sending it moves it here and removes their copy \
                                 — close it instead and nothing changes there."
                            </div>
                        </Show>
                        <div class="field-row">
                            <span class="field-label">"From"</span>
                            <span style="font-weight:500">{current_email}</span>
                            <span class="dot" style=move || format!("background:{}", current_color())></span>
                        </div>
                        {recipient_field(compose, to_input, to_suggest, account, "To", "name@example.com", |d| &d.to, |d, v| d.to = v)}
                        {recipient_field(compose, cc_input, cc_suggest, account, "Cc", "", |d| &d.cc, |d, v| d.cc = v)}
                        <div class="field-row">
                            <span class="field-label">"Subject"</span>
                            <input placeholder="Subject" prop:value=move || compose.get().map(|d| d.subject).unwrap_or_default()
                                on:input=move |e| compose.update(|c| if let Some(c) = c { c.subject = event_target_value(&e); })/>
                        </div>
                        <textarea placeholder="Write your message…" prop:value=move || compose.get().map(|d| d.body).unwrap_or_default()
                            on:input=move |e| compose.update(|c| if let Some(c) = c { c.body = event_target_value(&e); })></textarea>
                        <Show when=move || !attach_paths.get().is_empty()>
                            <div class="attach-row">
                                <For each=move || attach_paths.get() key=|p| p.clone() let:path>
                                    {
                                        let p = path.clone();
                                        let name = path.rsplit(['/', '\\']).next().unwrap_or(&path).to_owned();
                                        view! {
                                            <span class="chip attach">
                                                {icon(icons::CLIP)} {name}
                                                <span class="x" on:click=move |_| attach_paths.update(|l| l.retain(|x| x != &p))>{icon(icons::CLOSE)}</span>
                                            </span>
                                        }
                                    }
                                </For>
                            </div>
                        </Show>
                        <div class="compose-foot">
                            <button class="btn-primary" disabled=move || sending.get() on:click=move |_| send_compose()>
                                {move || if sending.get() { "Sending…" } else { "Send" }}
                            </button>
                            <button class="foot-attach" title="Attach files" on:click=move |_| attach_files()>
                                {icon(icons::CLIP)} "Attach"
                            </button>
                            <button class="foot-discard" title="Discard" on:click=move |_| discard_compose()>
                                {icon(icons::TRASH)} "Discard"
                            </button>
                            <button class="foot-md" class:on=move || md_on.get()
                                title="Format the message with Markdown"
                                on:click=move |_| md_on.update(|m| *m = !*m)>
                                {icon(icons::MARKDOWN)} "Markdown"
                            </button>
                            <button class="foot-save" title="Save as a draft to finish later"
                                on:click=move |_| save_current_draft()>
                                {icon(icons::DRAFTS)} "Save draft"
                            </button>
                        </div>
                    </div>
                </div>
            </Show>

            // ============ SETTINGS ============
            <Show when=move || settings_open.get()>
                <div class="scrim">
                    <div class="window settings">
                        <div class="settings-nav">
                            <div class="title">"Settings"</div>
                            {settings_tabs(settings_tab)}
                        </div>
                        <div class="settings-panel">
                            <div class="phead">
                                <div class="ptitle">{move || settings_tab_title(&settings_tab.get())}</div>
                                <button class="close-btn" style="margin-left:auto" on:click=move |_| settings_open.set(false)>{icon(icons::CLOSE)}</button>
                            </div>
                            <div class="settings-body">
                                // Accounts
                                <Show when=move || settings_tab.get() == "accounts">
                                    <div style="font-size:13px;color:var(--text-muted)">"Your accounts sync directly with each provider. Nothing passes through us."</div>
                                    <div style="display:flex;flex-direction:column;gap:10px;margin-top:18px">
                                        <For each=accounts_indexed key=|(_, a)| a.id let:pair>
                                            {
                                                let (i, a) = pair;
                                                let (id, email) = (a.id, a.email.clone());
                                                let email2 = email.clone();
                                                view! {
                                                    <div class="acct-card">
                                                        <div class="avatar">{email.chars().next().map(|c| c.to_ascii_uppercase().to_string()).unwrap_or_default()}</div>
                                                        <div style="min-width:0">
                                                            <div style="font-weight:600;font-size:14px">{email}</div>
                                                            <div class="sub"><span class="dot" style=format!("background:{}", account_color(i))></span>"IMAP · signed in"</div>
                                                        </div>
                                                        <span class="remove-link" on:click=move |_| confirm.set(Some((id, email2.clone())))>"Remove"</span>
                                                    </div>
                                                }
                                            }
                                        </For>
                                    </div>
                                    <button class="btn-ghost" style="margin-top:14px;display:inline-flex;align-items:center;gap:8px" on:click=move |_| open_wizard()>{icon(icons::PLUS)} "Add account"</button>
                                    <div style="margin-top:22px;padding-top:18px;border-top:1px solid var(--divider)">
                                        <div style="font-size:13px;font-weight:600;color:var(--text-muted);margin-bottom:8px">"Signature"</div>
                                        <textarea class="sig-box" prop:value=move || sig_text.get()
                                            on:change=move |e| save_signature(event_target_value(&e))></textarea>
                                    </div>
                                </Show>
                                // General
                                <Show when=move || settings_tab.get() == "general">
                                    <div class="setting-row">
                                        <div><div class="setting-name">"Mark as read when opened"</div><div class="setting-desc">"Turn off to mark read manually."</div></div>
                                        <div class="toggle" class:on=move || mark_read.get() on:click=move |_| toggle_mark()><span class="knob"></span></div>
                                    </div>
                                </Show>
                                // Appearance
                                <Show when=move || settings_tab.get() == "appearance">
                                    <div class="setting-row">
                                        <span class="setting-name">"Theme"</span>
                                        <div class="seg">
                                            <span class:on=move || !dark.get() on:click=move |_| set_theme(false)>"Light"</span>
                                            <span class:on=move || dark.get() on:click=move |_| set_theme(true)>"Dark"</span>
                                        </div>
                                    </div>
                                </Show>
                                // Privacy
                                <Show when=move || settings_tab.get() == "privacy">
                                    <div class="setting-row">
                                        <div><div class="setting-name">"Block remote images"</div><div class="setting-desc">"Stops senders knowing you opened their mail."</div></div>
                                        <div class="toggle" class:on=move || block_remote.get() on:click=move |_| toggle_block()><span class="knob"></span></div>
                                    </div>
                                    <div class="setting-row">
                                        <div><div class="setting-name">"Sync drafts to your provider"</div><div class="setting-desc">"Off by default: drafts stay on this device, encrypted. Turn on to also keep them in your provider's Drafts folder, so other mail apps can see them."</div></div>
                                        <div class="toggle" class:on=move || sync_drafts.get() on:click=move |_| toggle_sync_drafts()><span class="knob"></span></div>
                                    </div>
                                    <div class="privacy-note">
                                        <span style="color:var(--success)">{icon(icons::CHECK)}</span>
                                        "No telemetry and no tracking — always. Nothing about how you use GeleitMail leaves your machine."
                                    </div>
                                </Show>
                                // Notifications
                                <Show when=move || settings_tab.get() == "notifications">
                                    <div class="setting-row">
                                        <div>
                                            <div class="setting-name">"Notify me about new mail"</div>
                                            <div class="setting-desc">"A quiet desktop notification when mail arrives — several at once are shown as one."</div>
                                        </div>
                                        <div class="toggle" class:on=move || notify.get() on:click=move |_| toggle_notify()><span class="knob"></span></div>
                                    </div>
                                    <Show when=move || notify.get()>
                                        <div class="setting-row">
                                            <div>
                                                <div class="setting-name">"Quiet hours"</div>
                                                <div class="setting-desc">"Stay silent overnight. Mail that arrives is still there in the morning — and so is one notification telling you about it."</div>
                                            </div>
                                            <div class="toggle" class:on=move || quiet_on.get() on:click=move |_| toggle_quiet()><span class="knob"></span></div>
                                        </div>
                                        <Show when=move || quiet_on.get()>
                                            <div class="setting-row quiet-row">
                                                <span class="setting-name">"From"</span>
                                                <input type="time" class="folder-input quiet-time" prop:value=move || quiet_from.get()
                                                    on:change=move |e| { quiet_from.set(event_target_value(&e)); save_quiet(); }/>
                                                <span class="setting-name">"until"</span>
                                                <input type="time" class="folder-input quiet-time" prop:value=move || quiet_to.get()
                                                    on:change=move |e| { quiet_to.set(event_target_value(&e)); save_quiet(); }/>
                                            </div>
                                            <Show when=move || quiet_bad.get()>
                                                <div class="setting-warn">"Choose two different times — otherwise quiet hours do nothing."</div>
                                            </Show>
                                        </Show>
                                        <Show when=move || { accounts.get().len() > 1 || notify_accounts.get().values().any(|on| !on) }>
                                            <div class="setting-sub">"Which accounts"</div>
                                            <For each=move || accounts.get() key=|a| a.id let:a>
                                                {
                                                    let (id, email) = (a.id, a.email.clone());
                                                    view! {
                                                        <div class="setting-row">
                                                            <span class="setting-name">{email}</span>
                                                            <div class="toggle"
                                                                class:on=move || notify_accounts.get().get(&id).copied().unwrap_or(true)
                                                                on:click=move |_| toggle_account_notify(id)>
                                                                <span class="knob"></span>
                                                            </div>
                                                        </div>
                                                    }
                                                }
                                            </For>
                                        </Show>
                                    </Show>
                                </Show>
                            </div>
                        </div>
                    </div>
                </div>
            </Show>

            // ============ ADD-ACCOUNT WIZARD ============
            <Show when=move || wiz.get().is_some()>
                <div class="scrim">
                    <div class="window wizard">
                        <div class="win-head">
                            <span class="win-title">"Add account"</span>
                            <button class="close-btn" style="margin-left:auto" on:click=move |_| wiz.set(None)>{icon(icons::CLOSE)}</button>
                        </div>
                        <div class="wiz-body">
                            <div class="wiz-h">"Which account?"</div>
                            <div class="wiz-sub">"Sign in and your mail is just there — we talk straight to your provider, no middleman."</div>
                            <div class="provider-grid" style="margin-top:18px">
                                <For each=|| ["Gmail","Outlook","GMX","Web.de","Yahoo","iCloud"] key=|n| *n let:name>
                                    <button class="provider-btn" on:click=move |_| pick_provider(name)>
                                        <span class="badge">{name.chars().next().unwrap().to_string()}</span>{name}
                                    </button>
                                </For>
                            </div>
                            <div class="wiz-hint">"Servers filled in for you — just add your email + password. (One-click OAuth is coming; use an app password for now.)"</div>
                            <div class="wiz-or"><span class="line"></span><span class="txt">"your details"</span><span class="line"></span></div>
                            <div style="display:flex;flex-direction:column;gap:12px">
                                {wiz_field(wiz, "Email address", "you@example.com", false, |f| &f.email, |f, v| f.email = v)}
                                {wiz_field(wiz, "Password", "••••••••", true, |f| &f.password, |f, v| f.password = v)}
                                <div class="wiz-cols">
                                    <div style="flex:1">{wiz_field(wiz, "IMAP server", "imap.example.com", false, |f| &f.imap_host, |f, v| f.imap_host = v)}</div>
                                    <div style="width:88px;flex:none">{wiz_field(wiz, "Port", "993", false, |f| &f.imap_port, |f, v| f.imap_port = v)}</div>
                                </div>
                                <div class="wiz-cols">
                                    <div style="flex:1">{wiz_field(wiz, "SMTP server", "smtp.example.com", false, |f| &f.smtp_host, |f, v| f.smtp_host = v)}</div>
                                    <div style="width:88px;flex:none">{wiz_field(wiz, "Port", "465", false, |f| &f.smtp_port, |f, v| f.smtp_port = v)}</div>
                                </div>
                                <label class="setting-row" style="gap:8px;font-size:13px">
                                    <input type="checkbox" prop:checked=move || wiz.get().map(|f| f.smtp_starttls).unwrap_or(false)
                                        on:change=move |e| {
                                            let on = event_target_checked(&e);
                                            wiz.update(|f| if let Some(f) = f {
                                                f.smtp_starttls = on;
                                                if on && f.smtp_port == "465" { f.smtp_port = "587".into(); }
                                                if !on && f.smtp_port == "587" { f.smtp_port = "465".into(); }
                                            });
                                        }/>
                                    "Use STARTTLS for outgoing mail"
                                </label>
                                <Show when=move || wiz_note.get().is_some()>
                                    <div class="wiz-note ok"><span style="color:var(--accent-strong)">{icon(icons::CHECK)}</span>{move || wiz_note.get().unwrap_or_default()}</div>
                                </Show>
                            </div>
                            <div class="wiz-foot">
                                <span style="font-size:12px;color:var(--text-faint);display:inline-flex;align-items:center;gap:6px">{icon(icons::SHIELD)} "Stored in your keychain"</span>
                                <button class="btn-ghost" style="margin-left:auto" on:click=move |_| wiz.set(None)>"Cancel"</button>
                                <button class="btn-primary" disabled=move || connecting.get() on:click=move |_| submit_wizard()>
                                    {move || if connecting.get() { "Connecting…" } else { "Add account" }}
                                </button>
                            </div>
                        </div>
                    </div>
                </div>
            </Show>

            <Show when=move || folder_form.get().is_some()>
                <div class="scrim">
                    <div class="window dialog">
                        <h2>{move || if folder_form.get().and_then(|f| f.rename_from).is_some() { "Rename folder" } else { "New folder" }}</h2>
                        <input class="folder-input" placeholder="Folder name" autofocus
                            prop:value=move || folder_form.get().map(|f| f.name).unwrap_or_default()
                            on:input=move |e| folder_form.update(|f| { if let Some(f) = f { f.name = event_target_value(&e); } })
                            on:keydown=move |e| { if e.key() == "Enter" { submit_folder(); } }/>
                        <div class="drow">
                            <button class="btn-ghost" on:click=move |_| folder_form.set(None)>"Cancel"</button>
                            <button class="btn-primary"
                                disabled=move || folder_form.get().map(|f| f.name.trim().is_empty()).unwrap_or(true)
                                on:click=move |_| submit_folder()>
                                {move || if folder_form.get().and_then(|f| f.rename_from).is_some() { "Rename" } else { "Create" }}
                            </button>
                        </div>
                    </div>
                </div>
            </Show>

            <Show when=move || draft_del.get().is_some()>
                <div class="scrim">
                    <div class="window dialog">
                        <h2>"Delete this draft?"</h2>
                        <p>
                            "This draft is on your provider, not on this device. Deleting it removes it there \
                             for good — it isn't moved to Trash, and it can't be undone."
                        </p>
                        <div class="drow">
                            <button class="btn-ghost" on:click=move |_| draft_del.set(None)>"Cancel"</button>
                            <button class="btn-danger" on:click=move |_| do_remove_server_draft()>"Delete draft"</button>
                        </div>
                    </div>
                </div>
            </Show>

            <Show when=move || formatted_ask.get().is_some()>
                <div class="scrim">
                    <div class="window dialog">
                        <h2>"Continue this draft?"</h2>
                        <p>
                            "This draft was written with formatting. GeleitMail writes plain text, so \
                             continuing it keeps every word and drops the styling. Saving or sending it \
                             then removes the copy on your provider, so the formatted version is gone."
                        </p>
                        <div class="drow">
                            <button class="btn-ghost" on:click=move |_| formatted_ask.set(None)>"Cancel"</button>
                            <button class="btn" on:click=move |_| { if let Some(id) = formatted_ask.get() { resume_server_draft(id); } }>"Continue"</button>
                        </div>
                    </div>
                </div>
            </Show>

            <Show when=move || folder_del.get().is_some()>
                <div class="scrim">
                    <div class="window dialog">
                        <h2>"Delete folder?"</h2>
                        <p>
                            {move || folder_del.get().map(|(_, n)| format!(
                                "\u{201c}{n}\u{201d} and all the messages in it will be permanently deleted \
                                 from the server and this device. This can't be undone."))}
                        </p>
                        <div class="drow">
                            <button class="btn-ghost" on:click=move |_| folder_del.set(None)>"Cancel"</button>
                            <button class="btn-danger" on:click=move |_| do_delete_folder()>"Delete folder"</button>
                        </div>
                    </div>
                </div>
            </Show>

            <Show when=move || confirm.get().is_some()>
                <div class="scrim">
                    <div class="window dialog">
                        <h2>"Remove account?"</h2>
                        <p>
                            {move || confirm.get().map(|(_, e)| format!(
                                "{e} and its downloaded mail will be removed from this device. \
                                 Your mail stays on the server. This can't be undone here."))}
                        </p>
                        <div class="drow">
                            <button class="btn-ghost" on:click=move |_| confirm.set(None)>"Cancel"</button>
                            <button class="btn-danger" on:click=move |_| do_remove()>"Remove"</button>
                        </div>
                    </div>
                </div>
            </Show>

            <Show when=move || trash_ask.get().is_some()>
                <div class="scrim">
                    <div class="window dialog">
                        {move || match trash_ask.get() {
                            Some(TrashAsk::Empty) => view! {
                                <>
                                    <h2>"Empty Trash?"</h2>
                                    <p>"Every message in Trash will be permanently deleted from the server and this device. This can't be undone."</p>
                                    <div class="drow">
                                        <button class="btn-ghost" on:click=move |_| trash_ask.set(None)>"Cancel"</button>
                                        <button class="btn-danger" on:click=move |_| do_empty_trash()>"Empty Trash"</button>
                                    </div>
                                </>
                            }.into_any(),
                            _ => view! {
                                <>
                                    <h2>"Delete forever?"</h2>
                                    <p>"This message will be permanently deleted from the server and this device. This can't be undone."</p>
                                    <div class="drow">
                                        <button class="btn-ghost" on:click=move |_| trash_ask.set(None)>"Cancel"</button>
                                        <button class="btn-danger" on:click=move |_| {
                                            if let Some(TrashAsk::DeleteOne(id)) = trash_ask.get() { do_delete_forever(id); }
                                        }>"Delete forever"</button>
                                    </div>
                                </>
                            }.into_any(),
                        }}
                    </div>
                </div>
            </Show>

            <Show when=move || error.get().is_some()>
                <div class="toast" role="alert">
                    {move || error.get().unwrap_or_default()}
                    <a class="undo" on:click=move |_| error.set(None)>"Dismiss"</a>
                </div>
            </Show>
            <Show when=move || toast.get().is_some() && error.get().is_none()>
                <div class="toast" role="status">
                    {icon(icons::CHECK)}
                    {move || toast.get().unwrap_or_default()}
                    <Show when=move || pending.get().is_some()>
                        <a class="undo" on:click=move |_| undo_pending()>"Undo"</a>
                    </Show>
                </div>
            </Show>
        </div>
    }
}

/// The settings tabs, in order: `(key, label, icon-svg)`.
const SETTINGS_TABS: [(&str, &str, &str); 5] = [
    ("accounts", "Accounts", icons::IC_ACCOUNTS),
    ("general", "General", icons::IC_GENERAL),
    ("appearance", "Appearance", icons::THEME),
    ("privacy", "Privacy", icons::IC_PRIVACY),
    ("notifications", "Notifications", icons::IC_BELL),
];

/// The heading shown for a settings tab.
fn settings_tab_title(tab: &str) -> &'static str {
    SETTINGS_TABS
        .iter()
        .find(|(k, _, _)| *k == tab)
        .map(|(_, t, _)| *t)
        .unwrap_or("Settings")
}

/// The left-hand settings navigation (one selectable item per tab).
fn settings_tabs(tab: RwSignal<String>) -> impl IntoView {
    view! {
        <For each=|| SETTINGS_TABS key=|(k, _, _)| *k let:item>
            {
                let (key, label, svg) = item;
                view! {
                    <div class="item" class:on=move || tab.get() == key
                        on:click=move |_| tab.set(key.to_string())>
                        {icon(svg)} {label}
                    </div>
                }
            }
        </For>
    }
}

/// A compose recipient row (To / Cc) rendered as removable chips plus an input for the next address.
/// Committed addresses live in the draft field (`get`/`set`); `input` holds the in-progress text and
/// becomes a chip on Enter, comma, or blur.
#[allow(clippy::too_many_arguments)]
fn recipient_field(
    compose: RwSignal<Option<ComposeDraft>>,
    input: RwSignal<String>,
    suggestions: RwSignal<Vec<String>>, // autocomplete matches (held at App scope, see the Esc handler)
    account: RwSignal<Option<i64>>,
    label: &'static str,
    placeholder: &'static str,
    get: fn(&ComposeDraft) -> &String,
    set: fn(&mut ComposeDraft, String),
) -> impl IntoView {
    let field = move || compose.get().map(|d| get(&d).clone()).unwrap_or_default();
    let commit = move || {
        let typed = input.get_untracked();
        suggestions.set(Vec::new());
        if typed.trim().is_empty() {
            return;
        }
        compose.update(|c| {
            if let Some(c) = c {
                // Merge in the typed address(es), de-duplicating so the chip list (keyed off the
                // address in its `<For>`) stays unique.
                set(c, merge_addrs(get(c), &typed));
            }
        });
        input.set(String::new());
    };
    // Pick a suggestion: add it as a chip and close the dropdown (bypasses the typed text entirely).
    let select = move |addr: String| {
        compose.update(|c| {
            if let Some(c) = c {
                set(c, merge_addrs(get(c), &addr));
            }
        });
        input.set(String::new());
        suggestions.set(Vec::new());
    };
    // Look up past senders whenever the input text changes (typed or set programmatically), dropping
    // what's already chipped. Reads `compose`/`account` untracked so only `input` drives the effect;
    // the last-write-wins guard (`input` unchanged) keeps a slow lookup from clobbering newer keys.
    Effect::new(move |_| {
        let text = input.get();
        let Some(aid) = account.get_untracked() else {
            suggestions.set(Vec::new());
            return;
        };
        if text.trim().is_empty() {
            suggestions.set(Vec::new());
            return;
        }
        let already = split_addrs(
            &compose
                .get_untracked()
                .map(|d| get(&d).clone())
                .unwrap_or_default(),
        );
        leptos::task::spawn_local(async move {
            if let Ok(cands) = api::suggest_addresses(aid, text.clone()).await {
                if input.get_untracked() == text {
                    suggestions.set(rank_suggestions(&cands, &already, 6));
                }
            }
        });
    });
    let remove = move |addr: String| {
        compose.update(|c| {
            if let Some(c) = c {
                let kept = split_addrs(get(c))
                    .into_iter()
                    .filter(|a| a != &addr)
                    .collect::<Vec<_>>()
                    .join(", ");
                set(c, kept);
            }
        });
    };
    view! {
        <div class="field-row recipient">
            <span class="field-label">{label}</span>
            <For each=move || split_addrs(&field()) key=|a| a.clone() let:addr>
                {
                    let a = addr.clone();
                    view! {
                        <span class="chip">
                            {addr}
                            <span class="x" on:click=move |_| remove(a.clone())>{icon(icons::CLOSE)}</span>
                        </span>
                    }
                }
            </For>
            <input
                placeholder=placeholder
                prop:value=move || input.get()
                on:input=move |e| input.set(event_target_value(&e))
                on:keydown=move |e| {
                    // Escape is handled by the global document handler (it closes an open dropdown
                    // before it would discard the draft — this delegated listener fires too late).
                    let k = e.key();
                    if k == "Enter" || k == "," {
                        e.prevent_default();
                        commit();
                    }
                }
                on:blur=move |_| commit()
            />
            <Show when=move || !suggestions.get().is_empty()>
                <ul class="suggest">
                    <For each=move || suggestions.get() key=|a| a.clone() let:addr>
                        {
                            let a = addr.clone();
                            // mousedown (not click) with preventDefault: fires before the input's
                            // blur-commit and keeps focus, so picking a suggestion wins over the
                            // half-typed text.
                            view! {
                                <li on:mousedown=move |e| { e.prevent_default(); select(a.clone()); }>{addr}</li>
                            }
                        }
                    </For>
                </ul>
            </Show>
        </div>
    }
}

/// One labelled text field in the add-account wizard, two-way-bound to a field of the form.
fn wiz_field(
    wiz: RwSignal<Option<AccountForm>>,
    label: &'static str,
    placeholder: &'static str,
    is_password: bool,
    get: impl Fn(&AccountForm) -> &String + Send + 'static,
    set: impl Fn(&mut AccountForm, String) + Send + 'static,
) -> impl IntoView {
    view! {
        <div class="wiz-field">
            <div class="lab">{label}</div>
            <input
                type=if is_password { "password" } else { "text" }
                placeholder=placeholder
                prop:value=move || wiz.get().map(|f| get(&f).clone()).unwrap_or_default()
                on:input=move |e| {
                    let v = event_target_value(&e);
                    wiz.update(|w| if let Some(w) = w { set(w, v); });
                }
            />
        </div>
    }
}

/// Load an account's folders and open the first one.
async fn load_folders(
    account_id: i64,
    folders: RwSignal<Vec<Folder>>,
    selected: RwSignal<Option<i64>>,
    messages: RwSignal<Vec<Message>>,
    error: RwSignal<Option<String>>,
    request: RwSignal<u64>,
) {
    match api::list_folders(account_id).await {
        Ok(list) => {
            let first = list.first().map(|f| f.id);
            folders.set(list);
            if let Some(id) = first {
                selected.set(Some(id));
                let epoch = request.get_untracked() + 1;
                request.set(epoch);
                match api::list_messages(id, PAGE).await {
                    Ok(m) if request.get_untracked() == epoch => messages.set(m),
                    Ok(_) => {}
                    Err(e) => error.set(Some(e)),
                }
            }
        }
        Err(e) => error.set(Some(e)),
    }
}
