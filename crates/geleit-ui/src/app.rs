//! The three-pane shell: folder rail · message list · reading pane (S9.1).
//!
//! S9.1 deliberately renders the **plaintext** body only. HTML mail arrives in S9.2, confined to a
//! script-free, CSP-locked `<iframe>` — do not smuggle `html` into this document early: rendering
//! mail in the app's own document is exactly the thing ADR-0012's sandbox exists to prevent.
use crate::api::{self, Folder, Message, MessageBody};
use crate::view::{elide, format_date};
use leptos::prelude::*;

/// Wall-clock seconds, for relative date formatting. Reading the clock is the one impure thing the
/// view needs; [`format_date`] takes it as an argument so it stays testable.
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

    // Boot: first account → its folders → the first folder's messages.
    Effect::new(move |_| {
        leptos::task::spawn_local(async move {
            match api::list_accounts().await {
                Ok(accounts) => match accounts.first() {
                    Some(a) => {
                        account.set(Some(a.id));
                        load_folders(a.id, folders, selected_folder, messages, error).await;
                    }
                    None => loaded.set(true), // no account yet → calm empty state
                },
                Err(e) => error.set(Some(e)),
            }
            loaded.set(true);
        });
    });

    let choose_folder = move |id: i64| {
        selected_folder.set(Some(id));
        open.set(None);
        leptos::task::spawn_local(async move {
            match api::list_messages(id, PAGE).await {
                Ok(m) => messages.set(m),
                Err(e) => error.set(Some(e)),
            }
        });
    };

    let open_by_id = move |id: i64| {
        leptos::task::spawn_local(async move {
            match api::open_message(id).await {
                Ok(body) => {
                    open.set(Some(body));
                    // Opening a message marks it read (READ-7). Server write-back is S9.4's job;
                    // reflect it locally now so the dot clears instantly (P1 — never wait).
                    messages.update(|list| {
                        if let Some(m) = list.iter_mut().find(|m| m.id == id) {
                            m.seen = true;
                        }
                    });
                }
                Err(e) => error.set(Some(e)),
            }
        });
    };
    let choose_message = open_by_id;

    // Reconcile the theme against the store. index.html already painted an optimistic theme from
    // localStorage (it can't await IPC and still paint instantly), but the *store* is the source of
    // truth — that's the same row the Slint app writes, so a user's choice survives the migration.
    // We also refresh localStorage, so the next launch paints the right theme before first paint.
    Effect::new(move |_| {
        leptos::task::spawn_local(async move {
            if let Ok(Some(t)) = api::theme().await {
                apply_theme(&t);
            }
        });
    });

    // Dev/test seam (debug builds only): open a message on boot so the reading pane can be
    // screenshot-verified — the build environment can't inject clicks. No-op in release.
    Effect::new(move |_| {
        leptos::task::spawn_local(async move {
            if let Ok(Some(id)) = api::dev_open_message().await {
                open_by_id(id);
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
                <Show when=move || loaded.get() && messages.get().is_empty()>
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
                        let date = format_date(msg.date, now_secs());
                        let snippet = elide(&msg.snippet, 90);
                        let (from, subject) = (msg.from.clone(), msg.subject.clone());
                        let (flagged, attach) = (msg.flagged, msg.has_attachments);
                        view! {
                            <article
                                class="row"
                                class:unread=move || {
                                    messages.get().iter().find(|m| m.id == id).is_none_or(|m| !m.seen)
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
                    {move || open.get().map(|body| view! {
                        <>
                            <header class="read-head">
                                <h1>{body.subject.clone()}</h1>
                                <div class="meta">
                                    {body.from.clone()}
                                    <Show when={let d = body.date; move || d.is_some()}>
                                        " · " {format_date(body.date, now_secs())}
                                    </Show>
                                </div>
                            </header>
                            // S9.1 shows plaintext only. S9.2 replaces this with the sandboxed iframe.
                            <pre class="body">
                                {body.plain.clone().unwrap_or_else(|| {
                                    if body.html.is_some() {
                                        "This message is formatted (HTML). Formatted rendering arrives \
                                         in the next slice."
                                            .to_owned()
                                    } else {
                                        "This message has no text.".to_owned()
                                    }
                                })}
                            </pre>
                        </>
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
) {
    match api::list_folders(account_id).await {
        Ok(list) => {
            let first = list.first().map(|f| f.id);
            folders.set(list);
            if let Some(id) = first {
                selected.set(Some(id));
                match api::list_messages(id, PAGE).await {
                    Ok(m) => messages.set(m),
                    Err(e) => error.set(Some(e)),
                }
            }
        }
        Err(e) => error.set(Some(e)),
    }
}
