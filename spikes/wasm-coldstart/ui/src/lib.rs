//! Representative Leptos mail UI for the cold-start measurement. Deliberately exercises the things
//! the real UI would: a 3-pane layout, a 300-row list built from a signal, a selection signal that
//! drives a reading pane, a derived (memo) search filter, and per-row event handlers.
use leptos::prelude::*;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    /// Timing hook defined in index.html.
    fn mark(name: &str);
}

#[derive(Clone, PartialEq)] // Memo<Vec<Envelope>> needs PartialEq to skip no-op recomputes
struct Envelope {
    uid: u32,
    from: String,
    subject: String,
    preview: String,
    date: String,
    unread: bool,
}

fn demo_envelopes(n: u32) -> Vec<Envelope> {
    (0..n)
        .map(|i| Envelope {
            uid: i,
            from: format!("sender{i:03}@example.com"),
            subject: format!("Message {i} — quarterly summary and next steps"),
            preview: "Hi, following up on the thread from last week regarding the …".into(),
            date: format!("{:02}:{:02}", i % 24, i % 60),
            unread: i % 3 == 0,
        })
        .collect()
}

#[component]
fn App() -> impl IntoView {
    let envelopes = RwSignal::new(demo_envelopes(300));
    let selected = RwSignal::new(0u32);
    let query = RwSignal::new(String::new());

    // Derived/filtered view — the kind of reactive work the real list does.
    let visible = Memo::new(move |_| {
        let q = query.get().to_lowercase();
        envelopes.get()
            .into_iter()
            .filter(|e| q.is_empty() || e.subject.to_lowercase().contains(&q))
            .collect::<Vec<_>>()
    });

    let current = move || {
        envelopes.get()
            .into_iter()
            .find(|e| e.uid == selected.get())
    };

    view! {
        <div class="app">
            <nav class="rail">
                <div class="brand">"GeleitMail"</div>
                {["Inbox", "Sent", "Drafts", "Archive", "Spam", "Saved"]
                    .into_iter()
                    .map(|f| view! { <div class="folder">{f}</div> })
                    .collect_view()}
            </nav>

            <section class="list">
                <input
                    class="search"
                    placeholder="Search"
                    on:input=move |ev| query.set(event_target_value(&ev))
                />
                <For each=move || visible.get() key=|e| e.uid let:e>
                    {
                        let uid = e.uid;
                        view! {
                            <div
                                class="row"
                                class:unread=e.unread
                                class:sel=move || selected.get() == uid
                                on:click=move |_| selected.set(uid)
                            >
                                <div class="from">{e.from.clone()}</div>
                                <div class="subj">{e.subject.clone()}</div>
                                <div class="prev">{e.preview.clone()}</div>
                                <div class="date">{e.date.clone()}</div>
                            </div>
                        }
                    }
                </For>
            </section>

            <article class="read">
                {move || current().map(|e| view! {
                    <>
                        <h1>{e.subject}</h1>
                        <div class="meta">{e.from} " · " {e.date}</div>
                        <p>{e.preview}</p>
                    </>
                })}
            </article>
        </div>
    }
}

#[wasm_bindgen(start)]
pub fn start() {
    mark("wasm_start"); // wasm module instantiated, Rust entrypoint reached
    mount_to_body(App);
    mark("mounted"); // DOM built
}
