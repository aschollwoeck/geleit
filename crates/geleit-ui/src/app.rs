//! The three-pane shell: folder rail · message list · reading pane (S9.1).
//!
//! S9.1 deliberately renders the **plaintext** body only. HTML mail arrives in S9.2, confined to a
//! script-free, CSP-locked `<iframe>` — do not smuggle `html` into this document early: rendering
//! mail in the app's own document is exactly the thing ADR-0012's sandbox exists to prevent.
use crate::api::{self, AccountForm, ComposeDraft, Folder, Message, MessageBody};
use crate::view::{elide, format_date, visible_range};
use leptos::either::Either;
use leptos::prelude::*;
use std::collections::HashSet;

/// Fixed row height (px), matched by `.row` in the stylesheet. Virtualization needs a known height to
/// map scroll offset → row index; a fixed height keeps that exact and the list smooth.
const ROW_H: f64 = 64.0;

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

/// Seconds to add to a UTC timestamp to get **local** wall-clock time, for the instant `ts`.
///
/// Mail dates are UTC, but "09:30" has to mean 09:30 *where the reader is*. Computed per timestamp
/// rather than once, so a message from before a daylight-saving change still shows the time it
/// actually arrived. (`getTimezoneOffset` returns minutes *behind* UTC, hence the negation.)
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

/// How many messages the list asks for. Virtualization lands in S9.3; until then a bounded page
/// keeps the DOM small rather than materializing a 50k-row mailbox.
const PAGE: i64 = 300;

/// Set the document theme, and remember it so the *next* launch can paint it before first paint
/// (index.html cannot await IPC without going blank for ~630 ms).
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

/// Whether the document is currently in dark mode (the `data-theme` early.js painted). Used to seed
/// the toggle before the store's persisted choice reconciles.
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

/// One labelled field in the add-account form. Factored out so the form stays readable; `get`/`set`
/// bridge the field to its slot in the `AccountForm` signal.
fn setup_field(
    label: &'static str,
    placeholder: &'static str,
    password: bool,
    get: impl Fn() -> String + Send + 'static,
    set: impl Fn(String) + 'static,
) -> impl IntoView {
    view! {
        <label class="setup-field">
            <span>{label}</span>
            <input
                class="field"
                type=if password { "password" } else { "text" }
                placeholder=placeholder
                prop:value=get
                on:input=move |e| set(event_target_value(&e))
            />
        </label>
    }
}

#[component]
pub fn App() -> impl IntoView {
    let account = RwSignal::new(Option::<i64>::None);
    let folders = RwSignal::new(Vec::<Folder>::new());
    let selected_folder = RwSignal::new(Option::<i64>::None);
    let messages = RwSignal::new(Vec::<Message>::new());
    let open = RwSignal::new(Option::<MessageBody>::None);
    let error = RwSignal::new(Option::<String>::None);
    // Distinguishes "still loading" from "genuinely empty" — an empty mailbox and a mailbox that
    // hasn't answered yet must not look the same (P3: calm, never ambiguous).
    let loaded = RwSignal::new(false);
    // Messages read *this session*. Kept apart from `messages` so that marking one read doesn't
    // touch the list signal: every row subscribes to what it reads, and a row reading `messages`
    // would clone the whole 300-row Vec on each notification — 300 rows × 300 clones per click.
    let read_now = RwSignal::new(HashSet::<i64>::new());
    // Discards a stale folder's reply. Click A then B quickly and A can land last, leaving A's
    // messages on screen under B's highlight — click a row and you'd open mail from a folder you
    // are not in. Only the newest request may write.
    let request = RwSignal::new(0u64);
    // PRIV-2 is strictly PER MESSAGE: opting one message in must never carry over to the next, or a
    // single click would quietly turn remote loading on for everything you read afterwards.
    let load_images = RwSignal::new(false);
    // Virtualization: the list's scroll offset and viewport height drive which rows exist in the DOM.
    let scroll_top = RwSignal::new(0.0_f64);
    let viewport_h = RwSignal::new(700.0_f64); // a sane first value before the first scroll/measure
    let list_ref = NodeRef::<leptos::html::Section>::new();
    // Sync state (S9.4): whether a refresh is in flight, and the background catch-up count.
    let refreshing = RwSignal::new(false);
    let catchup = RwSignal::new(Option::<i64>::None); // Some(n) while backfilling; None when idle
                                                      // Compose state (S9.5): Some while the compose overlay is open, holding the editable draft.
    let compose = RwSignal::new(Option::<ComposeDraft>::None);
    let sending = RwSignal::new(false);
    // Search (S9.6): the query text; empty = show the folder, non-empty = show results.
    let query = RwSignal::new(String::new());
    // Setup (S9.6): Some while the add-account overlay is open, holding the editable form.
    let setup = RwSignal::new(Option::<AccountForm>::None);
    let connecting = RwSignal::new(false);
    // Seed from the theme early.js actually painted (it may have chosen dark from the OS preference
    // when the store has no setting yet), so the toggle's label is right before the store reconciles.
    let dark = RwSignal::new(document_is_dark());

    // Backfill progress streams in as `sync-progress` events; a negative value marks the catch-up
    // finished (-1 = ok, -2 = it stopped early). Either way, clear the strip and re-list.
    api::on_sync_progress(move |n| {
        if n < 0 {
            catchup.set(None);
            if n == -2 {
                error.set(Some(
                    "Couldn't finish catching up — will resume next refresh.".into(),
                ));
            }
            // The background catch-up pulled older mail — refresh the current view so it appears.
            // Re-run the *search* if one is active (don't replace results with the folder), else
            // re-list the folder. Epoch-guarded: a late reply must not clobber a newer view.
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

    // Boot: first account → its folders → the first folder's messages.
    Effect::new(move |_| {
        leptos::task::spawn_local(async move {
            match api::list_accounts().await {
                Ok(accounts) => match accounts.first() {
                    Some(a) => {
                        account.set(Some(a.id));
                        load_folders(a.id, folders, selected_folder, messages, error, request)
                            .await;
                        loaded.set(true);
                    }
                    None => loaded.set(true), // no account yet → calm empty state
                },
                // NOT `loaded` — a store failure must not be dressed up as "you have no account".
                // The user would be calmly invited to re-add an account that exists perfectly well.
                Err(e) => error.set(Some(e)),
            }
        });
    });

    let choose_folder = move |id: i64| {
        selected_folder.set(Some(id));
        open.set(None);
        query.set(String::new()); // switching folders leaves search mode; don't strand a stale term
        messages.set(Vec::new()); // don't show the previous folder's mail under the new folder's name
        let epoch = request.get_untracked() + 1;
        request.set(epoch);
        leptos::task::spawn_local(async move {
            match api::list_messages(id, PAGE).await {
                Ok(m) => {
                    if request.get_untracked() == epoch {
                        messages.set(m);
                    }
                }
                Err(e) => error.set(Some(e)),
            }
        });
    };

    let open_by_id = move |id: i64| {
        load_images.set(false); // a new message starts blocked, always
        leptos::task::spawn_local(async move {
            match api::open_message(id).await {
                Ok(body) => {
                    open.set(Some(body));
                    // The shell persisted `seen`; reflect it immediately so the dot clears without
                    // waiting for a re-list (P1 — the UI never waits on a round-trip it can predict).
                    read_now.update(|s| {
                        s.insert(id);
                    });
                }
                Err(e) => error.set(Some(e)),
            }
        });
    };
    let choose_message = open_by_id;

    // Star / unstar the currently-open message (optimistic; the shell writes back to the server).
    let toggle_star = move || {
        let Some(id) = open.get().map(|b| b.id) else {
            return;
        };
        let now_on = !messages
            .get_untracked()
            .iter()
            .find(|m| m.id == id)
            .map(|m| m.flagged)
            .unwrap_or(false);
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

    // Mark the open message unread again (brings the dot back; persisted + written back).
    let mark_unread = move || {
        let Some(id) = open.get().map(|b| b.id) else {
            return;
        };
        read_now.update(|s| {
            s.remove(&id);
        });
        messages.update(|list| {
            if let Some(m) = list.iter_mut().find(|m| m.id == id) {
                m.seen = false;
            }
        });
        leptos::task::spawn_local(async move {
            if let Err(e) = api::set_unread(id).await {
                error.set(Some(e));
            }
        });
    };

    // Archive / trash / spam the open message: remove it from the list now, close the pane, and let
    // the shell move it on the server. If the account has no such folder, restore nothing changed.
    let move_open = move |role: &'static str| {
        let Some(id) = open.get().map(|b| b.id) else {
            return;
        };
        let snapshot = messages.get_untracked();
        messages.update(|list| list.retain(|m| m.id != id));
        open.set(None);
        leptos::task::spawn_local(async move {
            match api::move_to_role(id, role).await {
                Ok(true) => {}
                Ok(false) => {
                    // no such folder — nothing moved; put the row back so the list stays truthful
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

    // Measure the list viewport once it's mounted, so a viewport taller than the initial estimate
    // renders fully from the start rather than only after the first scroll.
    Effect::new(move |_| {
        if let Some(el) = list_ref.get() {
            viewport_h.set(el.client_height() as f64);
        }
    });

    // Search (S9.6): run the store's FTS5 search; empty query returns to the current folder. Guarded
    // by the request epoch so a slow search can't clobber a folder the user switched to.
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

    // Theme toggle (S9.6): flip the document now (instant) and persist to the store.
    let toggle_theme = move || {
        let next = if dark.get() { "light" } else { "dark" };
        dark.set(next == "dark");
        apply_theme(next);
        let next = next.to_owned();
        leptos::task::spawn_local(async move {
            let _ = api::set_theme(&next).await;
        });
    };

    // Add-account overlay (S9.6): open a blank form with sensible defaults.
    let open_setup = move || {
        setup.set(Some(AccountForm {
            imap_port: "993".into(),
            smtp_port: "465".into(),
            ..AccountForm::default()
        }));
    };

    // Submit the setup form: validate + connect + first sync on a worker; on success, load the new
    // account's mail.
    let submit_setup = move || {
        let Some(form) = setup.get() else { return };
        if connecting.get() {
            return;
        }
        connecting.set(true);
        leptos::task::spawn_local(async move {
            match api::add_account(&form).await {
                Ok(aid) => {
                    setup.set(None);
                    account.set(Some(aid));
                    load_folders(aid, folders, selected_folder, messages, error, request).await;
                    loaded.set(true);
                }
                Err(e) => error.set(Some(e)),
            }
            connecting.set(false);
        });
    };

    // Open a blank compose window (New message).
    let compose_new = move || compose.set(Some(ComposeDraft::default()));

    // Open compose prefilled for reply / reply_all / forward of the open message.
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

    // Send the current compose draft. Runs on a worker (P1); closes the overlay on success.
    let send_compose = move || {
        let (Some(aid), Some(d)) = (account.get(), compose.get()) else {
            return;
        };
        if sending.get() {
            return; // already sending (the button is disabled too)
        }
        // A recipient in To *or* Cc is enough — the engine accepts a Cc-only message.
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
                Ok(()) => compose.set(None),
                Err(e) => error.set(Some(e)),
            }
            sending.set(false);
        });
    };

    // Refresh: sync recent mail (await), then the backfill streams progress via the event listener
    // above. Reloads the list when the recent sync lands. Never blocks the UI (P1).
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
        // Block a new refresh while one is in flight OR its background backfill is still streaming
        // (catchup is Some) — two overlapping backfills would interleave counts into one strip and
        // clear it prematurely.
        if refreshing.get() || catchup.get().is_some() || folder_name.is_empty() {
            return;
        }
        refreshing.set(true);
        catchup.set(Some(0));
        // Guard the post-sync re-list with the request epoch: if the user switches folders during
        // the multi-second sync, the resolved list must not clobber the newer folder's mail.
        let epoch = request.get_untracked() + 1;
        request.set(epoch);
        let q = query.get_untracked();
        leptos::task::spawn_local(async move {
            match api::refresh(aid, &folder_name).await {
                Ok(()) => {
                    // Refresh the current view — search results if a search is active, else the folder.
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
                }
                Err(e) => {
                    catchup.set(None);
                    error.set(Some(e));
                }
            }
            refreshing.set(false);
        });
    };

    // Reconcile the theme against the store. index.html already painted an optimistic theme from
    // localStorage (it can't await IPC and still paint instantly), but the *store* is the source of
    // truth — the same row the Slint app writes — so a user's choice survives the migration.
    Effect::new(move |_| {
        leptos::task::spawn_local(async move {
            if let Ok(Some(t)) = api::theme().await {
                dark.set(t == "dark");
                apply_theme(&t);
            }
        });
    });

    // Dev/test seam (debug builds only): open a message on boot so the reading pane can be
    // screenshot-verified — the build environment can't inject clicks. In release the command
    // doesn't exist, and the resulting error is ignored here.
    Effect::new(move |_| {
        leptos::task::spawn_local(async move {
            if let Ok(Some(id)) = api::dev_open_message().await {
                open_by_id(id);
                if api::dev_load_images().await.unwrap_or(false) {
                    load_images.set(true);
                }
            }
            if api::dev_setup().await.unwrap_or(false) {
                open_setup();
            }
            if let Ok(Some(kind)) = api::dev_compose().await {
                if kind == "new" {
                    compose_new();
                } else {
                    compose_from_open(match kind.as_str() {
                        "reply" => "reply",
                        "reply_all" => "reply_all",
                        _ => "forward",
                    });
                }
            }
        });
    });

    view! {
        <div class="app">
            <nav class="rail">
                <div class="brand">"GeleitMail"</div>
                <button class="compose-new" on:click=move |_| compose_new()>"✎ New message"</button>
                <div class="rail-tools">
                    <button class="tool" title="Add account" on:click=move |_| open_setup()>"＋ Account"</button>
                    <button class="tool" title="Toggle theme" on:click=move |_| toggle_theme()>
                        {move || if dark.get() { "☀ Light" } else { "☾ Dark" }}
                    </button>
                </div>
                <For each=move || folders.get() key=|f| f.id let:folder>
                    {
                        let (id, name) = (folder.id, folder.name.clone());
                        view! {
                            <button
                                class="folder"
                                class:sel=move || selected_folder.get() == Some(id)
                                on:click=move |_| choose_folder(id)
                            >
                                {name}
                            </button>
                        }
                    }
                </For>
            </nav>

            <div class="list-col">
            <div class="list-head">
                <input
                    class="search"
                    placeholder="Search mail"
                    prop:value=move || query.get()
                    on:input=move |e| run_search(event_target_value(&e))
                />
                <button
                    class="refresh"
                    disabled=move || refreshing.get() || catchup.get().is_some()
                    on:click=move |_| do_refresh()
                >
                    {move || if refreshing.get() { "Refreshing…" } else { "Refresh" }}
                </button>
                <Show when=move || catchup.get().is_some()>
                    <span class="catchup">
                        {move || match catchup.get() {
                            Some(0) => "Checking for new mail…".to_owned(),
                            Some(n) => format!("Catching up… {n}"),
                            None => String::new(),
                        }}
                    </span>
                </Show>
            </div>
            <section
                class="list"
                node_ref=list_ref
                on:scroll=move |_| {
                    if let Some(el) = list_ref.get() {
                        scroll_top.set(el.scroll_top() as f64);
                        viewport_h.set(el.client_height() as f64);
                    }
                }
            >
                <Show when=move || loaded.get() && messages.get().is_empty() && error.get().is_none()>
                    <div class="empty">
                        <Show
                            when=move || account.get().is_none()
                            fallback=|| view! { <p>"Nothing here."</p> }
                        >
                            <p>"No account yet. Add one to start reading your mail."</p>
                            <button class="compose-new" on:click=move |_| open_setup()>
                                "Add account"
                            </button>
                        </Show>
                    </div>
                </Show>
                // The full scrollable height, so the scrollbar is correct even though only a window of
                // rows exists in the DOM. The window is translated down to its true offset.
                <div
                    class="list-sizer"
                    // `.with(len)` reads the length WITHOUT cloning the Vec — this closure re-runs on
                    // every scroll tick, and cloning the whole list here would defeat virtualization.
                    style:height=move || format!("{}px", messages.with(Vec::len) as f64 * ROW_H)
                >
                    <div
                        class="list-window"
                        style:transform=move || {
                            let total = messages.with(Vec::len);
                            let (first, _) =
                                visible_range(scroll_top.get(), viewport_h.get(), ROW_H, total);
                            format!("translateY({}px)", first as f64 * ROW_H)
                        }
                    >
                        {move || {
                            let total = messages.with(Vec::len);
                            let (first, count) =
                                visible_range(scroll_top.get(), viewport_h.get(), ROW_H, total);
                            // Clone ONLY the visible window (~23 rows), never the whole list — the
                            // point of virtualization, and this runs on every scroll tick.
                            let window: Vec<Message> = messages
                                .with(|all| all[first..first + count].to_vec());
                            window
                                .into_iter()
                                .map(|msg| {
                                    let id = msg.id;
                                    let was_seen = msg.seen;
                                    let date = local_date(msg.date);
                                    let snippet = elide(&msg.snippet, 90);
                                    let (from, subject) = (msg.from.clone(), msg.subject.clone());
                                    let (flagged, attach) = (msg.flagged, msg.has_attachments);
                                    let convo = msg.thread_count;
                                    view! {
                                        <article
                                            class="row"
                                            class:unread=move || {
                                                !was_seen && !read_now.with(|s| s.contains(&id))
                                            }
                                            class:sel=move || open.get().is_some_and(|b| b.id == id)
                                            on:click=move |_| choose_message(id)
                                        >
                                            <div class="row-top">
                                                <span class="dot" aria-hidden="true"></span>
                                                <span class="from">{from}</span>
                                                <span class="marks">
                                                    <Show when=move || attach>
                                                        <span title="Has attachments">"📎"</span>
                                                    </Show>
                                                    <Show when=move || flagged>
                                                        <span class="star" title="Starred">"★"</span>
                                                    </Show>
                                                </span>
                                                <span class="date">{date}</span>
                                            </div>
                                            <div class="subj">{subject}</div>
                                            <div class="row-bottom">
                                                <span class="prev">{snippet}</span>
                                                <Show when=move || { convo > 1 }>
                                                    <span class="convo">
                                                        {format!("conversation · {convo}")}
                                                    </span>
                                                </Show>
                                            </div>
                                        </article>
                                    }
                                })
                                .collect_view()
                        }}
                    </div>
                </div>
            </section>
            </div>

            <main class="read">
                <Show
                    when=move || open.get().is_some()
                    fallback=|| view! { <p class="empty">"Select a message to read it."</p> }
                >
                    {move || open.get().map(|body| {
                        let id = body.id;
                        view! {
                            <>
                                <header class="read-head">
                                    <h1>{body.subject.clone()}</h1>
                                    <div class="meta">
                                        {body.from.clone()}
                                        {body.date.map(|_| " · ".to_owned())}
                                        {local_date(body.date)}
                                    </div>
                                    <div class="actions">
                                        <button title="Reply" on:click=move |_| compose_from_open("reply")>"Reply"</button>
                                        <button title="Reply all" on:click=move |_| compose_from_open("reply_all")>"Reply all"</button>
                                        <button title="Forward" on:click=move |_| compose_from_open("forward")>"Forward"</button>
                                        <button title="Star" on:click=move |_| toggle_star()>"★ Star"</button>
                                        <button title="Archive" on:click=move |_| move_open("archive")>"Archive"</button>
                                        <button title="Delete" on:click=move |_| move_open("trash")>"Delete"</button>
                                        <button title="Mark as spam" on:click=move |_| move_open("spam")>"Spam"</button>
                                        <button title="Mark unread" on:click=move |_| mark_unread()>"Unread"</button>
                                    </div>
                                </header>

                                // PRIV-3: say plainly that something was withheld, and let the reader
                                // decide (PRIV-2). Nothing remote loads until this button is pressed.
                                <Show when=move || body.has_remote && !load_images.get()>
                                    <div class="cue">
                                        <span>"Remote content was blocked to protect your privacy."</span>
                                        <button on:click=move |_| load_images.set(true)>
                                            "Load images"
                                        </button>
                                    </div>
                                </Show>

                                {if body.is_html {
                                    // The message is served on its OWN origin (mail://) and confined:
                                    // no allow-scripts, no allow-same-origin -> it cannot run code,
                                    // reach this document, touch the IPC bridge, or read files.
                                    // allow-popups(+escape) lets a link click surface as a new-window
                                    // request, which the shell hands to the system browser.
                                    Either::Left(view! {
                                        <iframe
                                            class="mail"
                                            sandbox="allow-popups allow-popups-to-escape-sandbox"
                                            src=move || if load_images.get() {
                                                format!("mail://localhost/{id}?images=1")
                                            } else {
                                                format!("mail://localhost/{id}")
                                            }
                                        ></iframe>
                                    })
                                } else {
                                    Either::Right(view! {
                                        <pre class="body">
                                            {body.plain.clone()
                                                .unwrap_or_else(|| "This message has no text.".to_owned())}
                                        </pre>
                                    })
                                }}
                            </>
                        }
                    })}
                </Show>
            </main>

            // Compose overlay (S9.5) — the app's own document, a plain form. Never a webview: no
            // untrusted content is rendered here, so it needs no sandbox.
            <Show when=move || compose.get().is_some()>
                <div class="compose-scrim">
                    <div class="compose" role="dialog" aria-label="Compose message">
                        <div class="compose-head">
                            <span>{move || {
                                let s = compose.get().map(|d| d.subject).unwrap_or_default();
                                let s = s.to_lowercase();
                                if s.starts_with("re:") { "Reply" }
                                else if s.starts_with("fwd:") || s.starts_with("fw:") { "Forward" }
                                else { "New message" }
                            }}</span>
                            <button class="close" title="Discard" on:click=move |_| compose.set(None)>"✕"</button>
                        </div>
                        <input
                            class="field" placeholder="To"
                            prop:value=move || compose.get().map(|d| d.to).unwrap_or_default()
                            on:input=move |e| compose.update(|c| { if let Some(c) = c { c.to = event_target_value(&e); } })
                        />
                        <input
                            class="field" placeholder="Cc"
                            prop:value=move || compose.get().map(|d| d.cc).unwrap_or_default()
                            on:input=move |e| compose.update(|c| { if let Some(c) = c { c.cc = event_target_value(&e); } })
                        />
                        <input
                            class="field" placeholder="Subject"
                            prop:value=move || compose.get().map(|d| d.subject).unwrap_or_default()
                            on:input=move |e| compose.update(|c| { if let Some(c) = c { c.subject = event_target_value(&e); } })
                        />
                        <textarea
                            class="field body" placeholder="Write your message…"
                            prop:value=move || compose.get().map(|d| d.body).unwrap_or_default()
                            on:input=move |e| compose.update(|c| { if let Some(c) = c { c.body = event_target_value(&e); } })
                        ></textarea>
                        <div class="compose-foot">
                            <button
                                class="send"
                                disabled=move || sending.get()
                                on:click=move |_| send_compose()
                            >
                                {move || if sending.get() { "Sending…" } else { "Send" }}
                            </button>
                            <button class="cancel" on:click=move |_| compose.set(None)>"Cancel"</button>
                        </div>
                    </div>
                </div>
            </Show>

            // Add-account overlay (S9.6) — the app's own document, a plain form. Credentials go
            // straight to the shell → the OS keychain; they never touch a webview or a log.
            <Show when=move || setup.get().is_some()>
                <div class="compose-scrim">
                    <div class="compose setup" role="dialog" aria-label="Add account">
                        <div class="compose-head">
                            <span>"Add account"</span>
                            <button class="close" title="Cancel" on:click=move |_| setup.set(None)>"✕"</button>
                        </div>
                        <div class="setup-fields">
                            {setup_field("Email", "email@example.com", false,
                                move || setup.get().map(|f| f.email).unwrap_or_default(),
                                move |v| setup.update(|f| if let Some(f) = f { f.email = v; }))}
                            {setup_field("Display name (optional)", "Your name", false,
                                move || setup.get().map(|f| f.display_name).unwrap_or_default(),
                                move |v| setup.update(|f| if let Some(f) = f { f.display_name = v; }))}
                            {setup_field("IMAP host", "imap.example.com", false,
                                move || setup.get().map(|f| f.imap_host).unwrap_or_default(),
                                move |v| setup.update(|f| if let Some(f) = f { f.imap_host = v; }))}
                            {setup_field("IMAP port", "993", false,
                                move || setup.get().map(|f| f.imap_port).unwrap_or_default(),
                                move |v| setup.update(|f| if let Some(f) = f { f.imap_port = v; }))}
                            {setup_field("Username", "usually your email", false,
                                move || setup.get().map(|f| f.username).unwrap_or_default(),
                                move |v| setup.update(|f| if let Some(f) = f { f.username = v; }))}
                            {setup_field("Password", "", true,
                                move || setup.get().map(|f| f.password).unwrap_or_default(),
                                move |v| setup.update(|f| if let Some(f) = f { f.password = v; }))}
                            {setup_field("SMTP host", "smtp.example.com", false,
                                move || setup.get().map(|f| f.smtp_host).unwrap_or_default(),
                                move |v| setup.update(|f| if let Some(f) = f { f.smtp_host = v; }))}
                            {setup_field("SMTP port", "465", false,
                                move || setup.get().map(|f| f.smtp_port).unwrap_or_default(),
                                move |v| setup.update(|f| if let Some(f) = f { f.smtp_port = v; }))}
                            <label class="setup-check">
                                <input type="checkbox"
                                    prop:checked=move || setup.get().map(|f| f.smtp_starttls).unwrap_or(false)
                                    on:change=move |e| setup.update(|f| if let Some(f) = f {
                                        f.smtp_starttls = event_target_checked(&e);
                                        // Move the port to the standard for the chosen mode, but only if
                                        // it's still on the *other* mode's default — never stomp a port
                                        // the user typed. (Checking STARTTLS while the field still reads
                                        // the implicit-TLS default 465 would otherwise send STARTTLS to 465.)
                                        if f.smtp_starttls && f.smtp_port == "465" { f.smtp_port = "587".into(); }
                                        if !f.smtp_starttls && f.smtp_port == "587" { f.smtp_port = "465".into(); }
                                    })
                                />
                                "Use STARTTLS for outgoing mail (port 587)"
                            </label>
                        </div>
                        <div class="compose-foot">
                            <button class="send" disabled=move || connecting.get() on:click=move |_| submit_setup()>
                                {move || if connecting.get() { "Connecting…" } else { "Connect" }}
                            </button>
                            <button class="cancel" on:click=move |_| setup.set(None)>"Cancel"</button>
                        </div>
                    </div>
                </div>
            </Show>

            <Show when=move || error.get().is_some()>
                <div class="toast" role="alert">
                    {move || error.get().unwrap_or_default()}
                    <button on:click=move |_| error.set(None)>"Dismiss"</button>
                </div>
            </Show>
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
                    // a folder click during boot must win over the boot load, not be overwritten
                    Ok(m) if request.get_untracked() == epoch => messages.set(m),
                    Ok(_) => {}
                    Err(e) => error.set(Some(e)),
                }
            }
        }
        Err(e) => error.set(Some(e)),
    }
}
