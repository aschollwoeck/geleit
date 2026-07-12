//! `geleit-ui` — the Leptos frontend (M9, ADR-0012), compiled Rust → WASM.
//!
//! Reaches the engine **only** over the Tauri IPC seam ([`api`]); it depends on none of our crates,
//! so view code cannot touch the store even by accident (ADR-0003). Pure view logic lives in
//! [`view`] so it is testable on the host target rather than only in a browser.
pub mod api;
pub mod app;
pub mod icons;
pub mod view;

/// WASM entrypoint. Mounts the app into `#app`, replacing the static skeleton that `index.html`
/// painted while WebKit was still booting (~630 ms — see `docs/technical/tauri-webkit-spike.md`).
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() {
    let root = leptos::prelude::document()
        .get_element_by_id("app")
        .expect("index.html must provide #app");
    root.set_inner_html(""); // drop the skeleton
    leptos::mount::mount_to(wasm_bindgen::JsCast::unchecked_into(root), crate::app::App).forget();
}
