//! The frontend half of the IPC seam. Mirrors `geleit-shell::ipc`'s DTOs and calls its commands.
//!
//! This is the *only* data path the UI has. It deliberately depends on none of our crates — the
//! frontend cannot reach the store except through a command the shell chose to expose.
use serde::{Deserialize, Serialize};
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Account {
    pub id: i64,
    pub email: String,
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Folder {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Message {
    pub id: i64,
    pub subject: String,
    pub from: String,
    pub snippet: String,
    pub date: Option<i64>,
    pub seen: bool,
    pub flagged: bool,
    pub has_attachments: bool,
}

/// A message opened for reading.
///
/// The HTML body is deliberately **absent**: hostile markup never enters the app's document, not even
/// as a string. When `is_html`, the reading pane points a sandboxed `<iframe>` at `mail://localhost/<id>`
/// and the shell serves the sanitized message on its own origin (ADR-0012, S9.2).
#[derive(Debug, Clone, Deserialize, Default, PartialEq, Eq)]
pub struct MessageBody {
    pub id: i64,
    pub subject: String,
    pub from: String,
    pub date: Option<i64>,
    pub plain: Option<String>,
    pub is_html: bool,
    /// Remote content was blocked (PRIV-3) → show the cue + "Load images" (PRIV-2).
    pub has_remote: bool,
}

// Tauri looks up command arguments by their **camelCase** names on the JS side, so the payload must
// be camelCase even though the Rust command signatures are snake_case. (Replies come back as plain
// serde JSON — snake_case — which is why only the argument structs are renamed.)
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AccountArgs {
    account_id: i64,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FolderArgs {
    folder_id: i64,
    limit: i64,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct IdArgs {
    id: i64,
}
#[derive(Serialize)]
struct NoArgs {}

// Provided by the shim in index.html, which forwards to Tauri's global `invoke`. Keeping the shim in
// JS (rather than pulling a Tauri JS package) is what lets us stay npm-free.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_name = geleitInvoke, catch)]
    async fn geleit_invoke(cmd: &str, args: JsValue) -> Result<JsValue, JsValue>;
}

/// Call a shell command and decode its reply. Errors come back as the shell's calm, PII-free strings.
#[cfg(target_arch = "wasm32")]
async fn call<A: Serialize, T: for<'de> Deserialize<'de>>(
    cmd: &str,
    args: &A,
) -> Result<T, String> {
    let args = serde_wasm_bindgen::to_value(args).map_err(|_| "Couldn't talk to the mailbox.")?;
    let raw = geleit_invoke(cmd, args)
        .await
        .map_err(|e| js_error_text(&e))?;
    serde_wasm_bindgen::from_value(raw).map_err(|_| "The mailbox sent something unexpected.".into())
}

/// A rejected `invoke` carries the command's `Err(String)`. Fall back to a calm message rather than
/// leaking a raw JS exception at the user.
#[cfg(target_arch = "wasm32")]
fn js_error_text(e: &JsValue) -> String {
    e.as_string()
        .unwrap_or_else(|| "Couldn't reach the mailbox.".to_owned())
}

// On the host target there is no webview to call into. These stubs exist so the crate still
// *compiles* for host — which is what lets clippy and the test suite cover it in CI like any other
// crate. They are never reached in the app.
#[cfg(not(target_arch = "wasm32"))]
async fn call<A: Serialize, T: for<'de> Deserialize<'de>>(
    _cmd: &str,
    _args: &A,
) -> Result<T, String> {
    Err("IPC is only available inside the app window.".to_owned())
}

pub async fn list_accounts() -> Result<Vec<Account>, String> {
    call("list_accounts", &NoArgs {}).await
}

pub async fn list_folders(account_id: i64) -> Result<Vec<Folder>, String> {
    call("list_folders", &AccountArgs { account_id }).await
}

pub async fn list_messages(folder_id: i64, limit: i64) -> Result<Vec<Message>, String> {
    call("list_messages", &FolderArgs { folder_id, limit }).await
}

pub async fn open_message(id: i64) -> Result<MessageBody, String> {
    call("open_message", &IdArgs { id }).await
}

/// The persisted theme (`"dark"`/`"light"`), or `None` if the user never chose one. The store — not
/// localStorage — is the source of truth, so the choice survives the M9 migration.
pub async fn theme() -> Result<Option<String>, String> {
    call("theme", &NoArgs {}).await
}

/// Dev/test seam — see `geleit-shell::ipc::dev_open_message`. Always `None` in a release build.
pub async fn dev_open_message() -> Result<Option<i64>, String> {
    call("dev_open_message", &NoArgs {}).await
}

/// Dev/test seam — see `geleit-shell::ipc::dev_load_images`. Always `false` in a release build.
pub async fn dev_load_images() -> Result<bool, String> {
    call("dev_load_images", &NoArgs {}).await
}
