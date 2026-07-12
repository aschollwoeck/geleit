//! The "Soft daylight" three-pane desktop client (design-overhaul slice): folder rail · message list
//! · reading pane, plus compose / settings / add-account windows. Mail HTML is never rendered in this
//! document — it is confined to a sandboxed `mail://` iframe (ADR-0012).
use crate::api::{self, Account, AccountForm, ComposeDraft, Folder, Message, MessageBody};
use crate::icons::{self, icon};
use crate::view::{elide, format_date};
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
    let folders = RwSignal::new(Vec::<Folder>::new());
    let selected_folder = RwSignal::new(Option::<i64>::None);
    let messages = RwSignal::new(Vec::<Message>::new());
    let open = RwSignal::new(Option::<MessageBody>::None);
    let error = RwSignal::new(Option::<String>::None);
    let loaded = RwSignal::new(false);
    // Read/unread tracked this session in small sets, kept apart from `messages` so toggling one
    // doesn't clone the whole list. `read_now`: opened this session. `marked_unread`: explicitly
    // set unread (works even for a message that arrived already-read).
    let read_now = RwSignal::new(HashSet::<i64>::new());
    let marked_unread = RwSignal::new(HashSet::<i64>::new());
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
    let sending = RwSignal::new(false);
    let query = RwSignal::new(String::new());
    let search_open = RwSignal::new(false);
    let settings_open = RwSignal::new(false);
    let settings_tab = RwSignal::new("accounts".to_string());
    let confirm = RwSignal::new(Option::<(i64, String)>::None); // (account_id, email) to remove
    let toast = RwSignal::new(Option::<String>::None);
    let dark = RwSignal::new(document_is_dark());
    // settings-backed prefs
    let block_remote = RwSignal::new(true);
    let mark_read = RwSignal::new(true);
    let notify = RwSignal::new(false);
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
            if let Some(fid) = selected_folder.get_untracked() {
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
                    .unwrap_or(false),
            );
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

    let choose_message = move |id: i64| {
        load_images.set(!block_remote.get_untracked()); // block-off ⇒ show images by default
        move_menu.set(false);
        leptos::task::spawn_local(async move {
            match api::open_message(id).await {
                Ok(body) => {
                    open.set(Some(body));
                    // read this session; clears any earlier "mark unread" on the same message
                    read_now.update(|s| {
                        s.insert(id);
                    });
                    marked_unread.update(|s| {
                        s.remove(&id);
                    });
                }
                Err(e) => error.set(Some(e)),
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
            }
        });
    };

    // Archive / trash / spam the open message: remove it now, close the pane, move on the server, and
    // show a toast. (Undo of a committed server move is a named follow-up — see the spec.)
    let move_open = move |role: &'static str, verb: &'static str| {
        let Some(id) = open.get().map(|b| b.id) else {
            return;
        };
        let snapshot = messages.get_untracked();
        messages.update(|list| list.retain(|m| m.id != id));
        open.set(None);
        move_menu.set(false);
        leptos::task::spawn_local(async move {
            match api::move_to_role(id, role).await {
                Ok(true) => {
                    toast.set(Some(verb.to_owned()));
                }
                Ok(false) => {
                    messages.set(snapshot);
                    error.set(Some("This account has no folder for that.".to_owned()));
                }
                Err(e) => {
                    messages.set(snapshot);
                    error.set(Some(e));
                }
            }
        });
    };

    let run_search = move |q: String| {
        query.set(q.clone());
        let Some(aid) = account.get() else { return };
        let epoch = request.get_untracked() + 1;
        request.set(epoch);
        leptos::task::spawn_local(async move {
            let result = if q.trim().is_empty() {
                match selected_folder.get_untracked() {
                    Some(fid) => api::list_messages(fid, PAGE).await,
                    None => Ok(Vec::new()),
                }
            } else {
                api::search(aid, &q).await
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
                    if let Ok(list) = api::list_accounts().await {
                        accounts.set(list);
                    }
                    account.set(Some(aid));
                    load_folders(aid, folders, selected_folder, messages, error, request).await;
                    loaded.set(true);
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
    let save_signature = move |text: String| {
        sig_text.set(text.clone());
        if let Some(aid) = account.get() {
            leptos::task::spawn_local(async move {
                let _ = api::set_signature(aid, &text).await;
            });
        }
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
                    toast.set(Some("Account removed".into()));
                }
                Err(e) => error.set(Some(e)),
            }
        });
    };

    // compose
    let compose_new = move || compose.set(Some(ComposeDraft::default()));
    let compose_from_open = move |kind: &'static str| {
        let Some(id) = open.get().map(|b| b.id) else {
            return;
        };
        leptos::task::spawn_local(async move {
            match api::compose_draft(id, kind).await {
                Ok(d) => compose.set(Some(d)),
                Err(e) => error.set(Some(e)),
            }
        });
    };
    let send_compose = move || {
        let (Some(aid), Some(d)) = (account.get(), compose.get()) else {
            return;
        };
        if sending.get() {
            return;
        }
        if d.to.trim().is_empty() && d.cc.trim().is_empty() {
            error.set(Some("Add at least one recipient.".to_owned()));
            return;
        }
        sending.set(true);
        leptos::task::spawn_local(async move {
            match api::send_message(
                aid,
                d.to,
                d.cc,
                d.subject,
                d.body,
                d.in_reply_to,
                d.references,
            )
            .await
            {
                Ok(()) => {
                    compose.set(None);
                    toast.set(Some("Sent".into()));
                }
                Err(e) => error.set(Some(e)),
            }
            sending.set(false);
        });
    };

    let do_refresh = move || {
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

    // auto-dismiss the toast after ~4.5s, cancelling any previous timer so a fresh toast that
    // replaces an older one still gets its full lifetime (rather than being cleared early).
    #[cfg(target_arch = "wasm32")]
    let toast_timer = RwSignal::new(Option::<i32>::None);
    Effect::new(move |_| {
        if toast.get().is_some() {
            #[cfg(target_arch = "wasm32")]
            {
                if let Some(w) = web_sys::window() {
                    if let Some(prev) = toast_timer.get_untracked() {
                        w.clear_timeout_with_handle(prev);
                    }
                    let cb = wasm_bindgen::closure::Closure::once_into_js(move || toast.set(None));
                    if let Ok(id) = w.set_timeout_with_callback_and_timeout_and_arguments_0(
                        cb.as_ref().unchecked_ref(),
                        4500,
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
    Effect::new(move |prev: Option<()>| {
        if prev.is_some() {
            return;
        }
        leptos::task::spawn_local(async move {
            if let Ok(Some(id)) = api::dev_open_message().await {
                if api::dev_load_images().await.unwrap_or(false) {
                    load_images.set(true);
                }
                choose_message(id);
            }
            if let Ok(Some(kind)) = api::dev_compose().await {
                match kind.as_str() {
                    "new" => compose_new(),
                    "reply_all" => compose_from_open("reply_all"),
                    "forward" => compose_from_open("forward"),
                    _ => compose_from_open("reply"),
                }
            }
        });
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
                    if compose.get_untracked().is_some() {
                        compose.set(None);
                    } else if wiz.get_untracked().is_some() {
                        wiz.set(None);
                    } else if settings_open.get_untracked() {
                        settings_open.set(false);
                    } else if confirm.get_untracked().is_some() {
                        confirm.set(None);
                    } else if move_menu.get_untracked() || acct_menu.get_untracked() {
                        move_menu.set(false);
                        acct_menu.set(false);
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
                    "e" if open.get_untracked().is_some() => move_open("archive", "Archived"),
                    "#" if open.get_untracked().is_some() => move_open("trash", "Deleted"),
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
        let mut buckets: [(&'static str, Vec<Message>); 3] = [
            ("Today", Vec::new()),
            ("Yesterday", Vec::new()),
            ("Earlier", Vec::new()),
        ];
        for m in messages.get() {
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
                                let (id, name) = (f.id, f.name.clone());
                                view! {
                                    <button class="folder" class:active=move || selected_folder.get() == Some(id)
                                        title=name.clone() on:click=move |_| choose_folder(id)>
                                        <span class="guide"></span>
                                        <span class="fic">{icon(icons::folder_icon(&name))}</span>
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
                            <div class="avatar">{account_initial}</div>
                            <div style="min-width:0;flex:1">
                                <div class="acct-name">{current_email}</div>
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
                                <For each=accounts_indexed key=|(_, a)| a.id let:pair>
                                    {
                                        let (i, a) = pair;
                                        let (id, email) = (a.id, a.email.clone());
                                        view! {
                                            <div class="menu-item" class:sel=move || account.get() == Some(id)
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
                    <For each=move || folders.get() key=|f| f.id let:f>
                        {
                            let (id, name, unread) = (f.id, f.name.clone(), f.unread);
                            let label = name.clone();
                            view! {
                                <button class="folder" class:active=move || selected_folder.get() == Some(id)
                                    on:click=move |_| choose_folder(id)>
                                    <span class="guide"></span>
                                    <span class="fic">{icon(icons::folder_icon(&name))}</span>
                                    {label}
                                    <span class="fcount">{move || if unread > 0 { unread.to_string() } else { String::new() }}</span>
                                </button>
                            }
                        }
                    </For>
                    <div class="rail-fill"></div>
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
                    <div class="list-title">{move || folders.get().into_iter().find(|f| selected_folder.get() == Some(f.id)).map(|f| f.name).unwrap_or_default()}</div>
                    <div class="list-sub">{move || {
                        let n = messages.get().iter().filter(|m| is_unread(m.id, m.seen, read_now, marked_unread)).count();
                        if n > 0 { format!("{n} unread") } else { String::new() }
                    }}</div>
                    <div class="list-actions">
                        <span class="icon-btn" class:on=move || search_open.get() title="Search" on:click=move |_| search_open.update(|o| *o = !*o)>{icon(icons::SEARCH)}</span>
                        <span class="icon-btn" title="Refresh" on:click=move |_| do_refresh()>{icon(icons::REFRESH)}</span>
                    </div>
                </div>
                <Show when=move || refreshing.get() || catchup.get().is_some()>
                    <div style="padding:0 20px 8px;font-size:12px;color:var(--text-muted)">
                        {move || match catchup.get() {
                            Some(0) => "Checking for new mail…".to_owned(),
                            Some(n) => format!("Catching up… {n}"),
                            None => "Refreshing…".to_owned(),
                        }}
                    </div>
                </Show>
                <Show when=move || search_open.get()>
                    <div class="list-search">
                        <input placeholder="Search mail" prop:value=move || query.get()
                            on:input=move |e| run_search(event_target_value(&e))/>
                    </div>
                </Show>
                <div class="list-scroll">
                    <Show when=move || loaded.get() && messages.get().is_empty() && error.get().is_none()>
                        <div class="list-empty">
                            <Show when=move || account.get().is_none() fallback=|| view! { <div class="big">"✓"</div><div class="msg">"Nothing here."</div> }>
                                <div class="msg">"No account yet."</div>
                                <button class="btn-primary add" on:click=move |_| open_wizard()>"Add account"</button>
                            </Show>
                        </div>
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
                                Either::Right(view! {
                                    <div class="row"
                                        class:unread=move || is_unread(id, was_seen, read_now, marked_unread)
                                        class:sel=move || open.get().is_some_and(|b| b.id == id)
                                        on:click=move |_| choose_message(id)>
                                        <span class="guide"></span>
                                        <div class="row-top">
                                            <span class="udot"></span>
                                            <span class="sender">{from}</span>
                                            <Show when=move || attach>
                                                <span class="clip">{icon(icons::CLIP)}</span>
                                            </Show>
                                            <span class="time">{date}</span>
                                        </div>
                                        <div class="subj">{subject}</div>
                                        <div class="prev">{snippet}</div>
                                        <div class="acct">
                                            <span class="dot" style=move || format!("background:{}", current_color())></span>
                                            {current_email}
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
                                    <h1>{body.subject.clone()}</h1>
                                    <div class="read-from">
                                        <div class="read-avatar">{body.from.chars().next().map(|c| c.to_ascii_uppercase().to_string()).unwrap_or_default()}</div>
                                        <div class="read-sender">{body.from.clone()}</div>
                                        <div class="read-when">
                                            <span class="dot" style=move || format!("background:{}", current_color())></span>
                                            {local_date(body.date)}
                                        </div>
                                    </div>
                                    <div class="actions">
                                        <span class="act" on:click=move |_| compose_from_open("reply")>{icon(icons::REPLY)} "Reply"</span>
                                        <span class="act" on:click=move |_| compose_from_open("reply_all")>{icon(icons::REPLY_ALL)} "Reply all"</span>
                                        <span class="act" on:click=move |_| compose_from_open("forward")>{icon(icons::FORWARD)} "Forward"</span>
                                        <span class="act" on:click=move |_| move_open("archive", "Archived")>{icon(icons::ARCHIVE)} "Archive"</span>
                                        <span class="act" on:click=move |_| move_menu.update(|o| *o = !*o)>{icon(icons::MOVE)} "Move"</span>
                                        <span class="act danger" on:click=move |_| move_open("trash", "Deleted")>{icon(icons::TRASH)} "Delete"</span>
                                        <span class="act" on:click=move |_| mark_unread()>{icon(icons::UNREAD)} "Unread"</span>
                                        <Show when=move || move_menu.get()>
                                            <div class="menu" style="right:0;top:42px;width:180px">
                                                <For each=move || folders.get() key=|f| f.id let:f>
                                                    {
                                                        let name = f.name.clone();
                                                        let role = folder_role(&name);
                                                        view! {
                                                            <div class="menu-item" on:click=move |_| move_open(role, "Moved")>{name}</div>
                                                        }
                                                    }
                                                </For>
                                            </div>
                                        </Show>
                                    </div>
                                </div>
                                <Show when=move || body.has_remote && !load_images.get()>
                                    <div class="cue">
                                        <span class="edge"></span>
                                        {icon(icons::WARN)}
                                        "Remote content blocked"
                                        <a class="load" on:click=move |_| load_images.set(true)>"Load images"</a>
                                    </div>
                                </Show>
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
                            <button class="close-btn" on:click=move |_| compose.set(None)>{icon(icons::CLOSE)}</button>
                        </div>
                        <div class="field-row">
                            <span class="field-label">"From"</span>
                            <span style="font-weight:500">{current_email}</span>
                            <span class="dot" style=move || format!("background:{}", current_color())></span>
                        </div>
                        <div class="field-row">
                            <span class="field-label">"To"</span>
                            <input placeholder="name@example.com" prop:value=move || compose.get().map(|d| d.to).unwrap_or_default()
                                on:input=move |e| compose.update(|c| if let Some(c) = c { c.to = event_target_value(&e); })/>
                        </div>
                        <div class="field-row">
                            <span class="field-label">"Cc"</span>
                            <input placeholder="" prop:value=move || compose.get().map(|d| d.cc).unwrap_or_default()
                                on:input=move |e| compose.update(|c| if let Some(c) = c { c.cc = event_target_value(&e); })/>
                        </div>
                        <div class="field-row">
                            <span class="field-label">"Subject"</span>
                            <input placeholder="Subject" prop:value=move || compose.get().map(|d| d.subject).unwrap_or_default()
                                on:input=move |e| compose.update(|c| if let Some(c) = c { c.subject = event_target_value(&e); })/>
                        </div>
                        <textarea placeholder="Write your message…" prop:value=move || compose.get().map(|d| d.body).unwrap_or_default()
                            on:input=move |e| compose.update(|c| if let Some(c) = c { c.body = event_target_value(&e); })></textarea>
                        <div class="compose-foot">
                            <button class="btn-primary" disabled=move || sending.get() on:click=move |_| send_compose()>
                                {move || if sending.get() { "Sending…" } else { "Send" }}
                            </button>
                            <span class="draft-status">"Draft"</span>
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
                                    <div class="privacy-note">
                                        <span style="color:var(--success)">{icon(icons::CHECK)}</span>
                                        "No telemetry and no tracking — always. Nothing about how you use GeleitMail leaves your machine."
                                    </div>
                                </Show>
                                // Notifications
                                <Show when=move || settings_tab.get() == "notifications">
                                    <div class="setting-row">
                                        <span class="setting-name">"Notify me about new mail"</span>
                                        <div class="toggle" class:on=move || notify.get() on:click=move |_| toggle_notify()><span class="knob"></span></div>
                                    </div>
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

/// The role a folder maps to for [`api::move_to_role`], inferred from its name.
fn folder_role(name: &str) -> &'static str {
    let n = name.to_lowercase();
    if n.contains("archive") {
        "archive"
    } else if n.contains("trash") || n.contains("deleted") || n.contains("bin") {
        "trash"
    } else if n.contains("spam") || n.contains("junk") {
        "spam"
    } else {
        "inbox"
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
