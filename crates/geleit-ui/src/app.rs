//! The three-pane shell: folder rail · message list · reading pane (S9.1).
//!
//! S9.1 deliberately renders the **plaintext** body only. HTML mail arrives in S9.2, confined to a
//! script-free, CSP-locked `<iframe>` — do not smuggle `html` into this document early: rendering
//! mail in the app's own document is exactly the thing ADR-0012's sandbox exists to prevent.
use crate::api::{self, Folder, Message, MessageBody};
use crate::view::{elide, format_date};
use leptos::either::Either;
use leptos::prelude::*;
use std::collections::HashSet;

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

    // Reconcile the theme against the store. index.html already painted an optimistic theme from
    // localStorage (it can't await IPC and still paint instantly), but the *store* is the source of
    // truth — the same row the Slint app writes — so a user's choice survives the migration.
    Effect::new(move |_| {
        leptos::task::spawn_local(async move {
            if let Ok(Some(t)) = api::theme().await {
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
        });
    });

    view! {
        <div class="app">
            <nav class="rail">
                <div class="brand">"GeleitMail"</div>
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

            <section class="list">
                <Show when=move || loaded.get() && messages.get().is_empty() && error.get().is_none()>
                    <p class="empty">
                        {move || if account.get().is_none() {
                            "No account yet. Add one to start reading your mail."
                        } else {
                            "Nothing here."
                        }}
                    </p>
                </Show>
                <For each=move || messages.get() key=|m| m.id let:msg>
                    {
                        let id = msg.id;
                        let was_seen = msg.seen;
                        let date = local_date(msg.date);
                        let snippet = elide(&msg.snippet, 90);
                        let (from, subject) = (msg.from.clone(), msg.subject.clone());
                        let (flagged, attach) = (msg.flagged, msg.has_attachments);
                        view! {
                            <article
                                class="row"
                                // reads only the small read-this-session set, never the message list
                                class:unread=move || !was_seen && !read_now.with(|s| s.contains(&id))
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
                                <div class="prev">{snippet}</div>
                            </article>
                        }
                    }
                </For>
            </section>

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
