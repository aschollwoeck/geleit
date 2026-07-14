//! The frontend half of the IPC seam. Mirrors `geleit-app::ipc`'s DTOs and calls its commands.
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
    #[serde(default)]
    pub unread: i64,
    /// What the server says this folder is for (`drafts`, `sent`, `trash`, `archive`, `junk`,
    /// `inbox`), or `None` if it didn't say. A folder called `Entwürfe` still gets the drafts icon and
    /// the same protection from renaming — neither of which its *name* could tell us.
    #[serde(default)]
    pub role: Option<String>,
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
    /// Conversation size (READ-5); the UI shows `conversation · N` only when `> 1`.
    pub thread_count: u32,
    /// Owning account — only set in the merged "All inboxes" listing (0 otherwise).
    #[serde(default)]
    pub account: i64,
}

/// A compose form, prefilled for reply/forward or blank for a new message.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct ComposeDraft {
    pub to: String,
    pub cc: String,
    pub subject: String,
    pub body: String,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
}

/// A resumed draft: the form + the on-disk paths its saved attachments were materialised to.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct ResumedDraft {
    pub draft: ComposeDraft,
    pub attachments: Vec<String>,
}

/// A row in the Drafts list (mirrors `geleit-app::dto::DraftSummary`).
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct DraftSummary {
    pub id: i64,
    pub to: String,
    pub subject: String,
    pub snippet: String,
    pub updated_at: i64,
    /// This draft is in the provider's Drafts folder, not on this device — so `id` is a **message**
    /// id, and it's continued and deleted down different paths than a local draft.
    pub on_server: bool,
    /// Written with formatting (HTML). Continuing it in the plain-text composer drops the styling and
    /// replaces the original, so we ask first.
    pub formatted: bool,
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
    /// Attachments (name + human size); bytes are fetched on demand to save (READ-8).
    pub attachments: Vec<Attachment>,
}

/// One attachment in the reading pane (mirrors `geleit-app::dto::AttachmentDto`).
#[derive(Debug, Clone, Deserialize, Default, PartialEq, Eq)]
pub struct Attachment {
    pub name: String,
    pub size: String,
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
struct CreateFolderArgs {
    account_id: i64,
    name: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RenameFolderArgs {
    account_id: i64,
    from: String,
    to: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DeleteFolderArgs {
    account_id: i64,
    folder_id: i64,
    name: String,
}
#[derive(Serialize)]
struct LimitArgs {
    limit: i64,
}
#[derive(Serialize)]
struct QueryArg {
    query: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct IdArgs {
    id: i64,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct OpenArgs {
    id: i64,
    mark_read: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StarArgs {
    id: i64,
    on: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MoveArgs {
    id: i64,
    role: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RefreshArgs {
    account_id: i64,
    folder: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ComposeDraftArgs {
    id: i64,
    kind: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SendArgs {
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
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SaveDraftArgs {
    account_id: i64,
    draft_id: Option<i64>,
    draft: ComposeDraft,
    attachments: Vec<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SaveAttachmentArgs {
    message_id: i64,
    index: usize,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SuggestArgs {
    account_id: i64,
    prefix: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SearchArgs {
    account_id: i64,
    query: String,
}
#[derive(Serialize)]
struct ThemeArg {
    theme: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct KeyBoolArg {
    key: String,
    value: bool,
}
#[derive(Serialize)]
struct KeyArg {
    key: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SigArg {
    account_id: i64,
    signature: String,
}
/// The add-account form. camelCase to match the Tauri command parameters.
///
/// `Debug` is **hand-written** to redact the password — a derived `Debug` would print the credential
/// in the clear from one stray `{:?}` (P2: credentials are never logged).
#[derive(Serialize, Default, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AccountForm {
    pub email: String,
    pub display_name: String,
    pub imap_host: String,
    pub imap_port: String,
    pub username: String,
    pub password: String,
    pub smtp_host: String,
    pub smtp_port: String,
    pub smtp_starttls: bool,
    pub signature: String,
    pub allow_invalid_certs: bool,
}

impl std::fmt::Debug for AccountForm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AccountForm")
            .field("email", &self.email)
            .field("imap_host", &self.imap_host)
            .field("password", &"<redacted>")
            .finish_non_exhaustive()
    }
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
    #[wasm_bindgen(js_name = geleitOnSyncProgress)]
    fn geleit_on_sync_progress(cb: &wasm_bindgen::JsValue);
    #[wasm_bindgen(js_name = geleitOnMailArrived)]
    fn geleit_on_mail_arrived(cb: &wasm_bindgen::JsValue);
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

/// The merged "All inboxes" listing: every account's INBOX, newest first, tagged with its account.
pub async fn list_all_messages(limit: i64) -> Result<Vec<Message>, String> {
    call("list_all_messages", &LimitArgs { limit }).await
}

pub async fn open_message(id: i64, mark_read: bool) -> Result<MessageBody, String> {
    call("open_message", &OpenArgs { id, mark_read }).await
}

pub async fn set_star(id: i64, on: bool) -> Result<(), String> {
    call("set_star", &StarArgs { id, on }).await
}

pub async fn set_unread(id: i64) -> Result<(), String> {
    call("set_unread", &IdArgs { id }).await
}

/// Mark a message read (server + local) — for bulk mark-read.
pub async fn set_read(id: i64) -> Result<(), String> {
    call("set_read", &IdArgs { id }).await
}

/// Move a message to a role folder: "archive" | "trash" | "spam" | "inbox". Returns whether it acted
/// (false = the account has no such folder).
pub async fn move_to_folder(id: i64, folder: &str) -> Result<bool, String> {
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Args<'a> {
        id: i64,
        folder: &'a str,
    }
    call("move_to_folder", &Args { id, folder }).await
}

/// Move a message to a well-known folder by ROLE (archive / trash / spam / inbox) — the toolbar
/// actions, where GeleitMail has to find the folder itself.
pub async fn move_to_role(id: i64, role: &str) -> Result<bool, String> {
    call(
        "move_to_role",
        &MoveArgs {
            id,
            role: role.to_owned(),
        },
    )
    .await
}

/// Empty the account's Trash — permanently, server + local. Irreversible.
pub async fn empty_trash(account_id: i64) -> Result<(), String> {
    call("empty_trash", &AccountArgs { account_id }).await
}

/// Permanently delete a single message that's already in Trash. Irreversible.
pub async fn delete_forever(id: i64) -> Result<(), String> {
    call("delete_forever", &IdArgs { id }).await
}

/// Create a folder on the server + locally; returns the new folder id.
pub async fn create_folder(account_id: i64, name: String) -> Result<i64, String> {
    call("create_folder", &CreateFolderArgs { account_id, name }).await
}

/// Rename a folder (server + local, keeping its messages).
pub async fn rename_folder(account_id: i64, from: String, to: String) -> Result<(), String> {
    call(
        "rename_folder",
        &RenameFolderArgs {
            account_id,
            from,
            to,
        },
    )
    .await
}

/// Delete a folder (server + local, with its messages). Irreversible.
pub async fn delete_folder(account_id: i64, folder_id: i64, name: String) -> Result<(), String> {
    call(
        "delete_folder",
        &DeleteFolderArgs {
            account_id,
            folder_id,
            name,
        },
    )
    .await
}

/// Search every account's mail at once (for the merged "All inboxes" view), tagged with account.
pub async fn search_all(query: &str) -> Result<Vec<Message>, String> {
    call(
        "search_all",
        &QueryArg {
            query: query.to_owned(),
        },
    )
    .await
}

pub async fn search(account_id: i64, query: &str) -> Result<Vec<Message>, String> {
    call(
        "search",
        &SearchArgs {
            account_id,
            query: query.to_owned(),
        },
    )
    .await
}

/// Add (or reconnect) an account; returns its id. Worker on the shell side (network + keychain).
pub async fn add_account(form: &AccountForm) -> Result<i64, String> {
    call("add_account", form).await
}

/// Persist the theme choice so it survives restart (the frontend already flipped the document).
pub async fn set_theme(theme: &str) -> Result<(), String> {
    call(
        "set_theme",
        &ThemeArg {
            theme: theme.to_owned(),
        },
    )
    .await
}

/// The persisted theme (`"dark"`/`"light"`), or `None` if the user never chose one. The store — not
/// localStorage — is the source of truth, so the choice survives the M9 migration.
pub async fn theme() -> Result<Option<String>, String> {
    call("theme", &NoArgs {}).await
}

/// Build a reply/reply-all/forward draft prefilled from a stored message. `kind` = "reply" |
/// "reply_all" | "forward".
pub async fn compose_draft(id: i64, kind: &str) -> Result<ComposeDraft, String> {
    call(
        "compose_draft",
        &ComposeDraftArgs {
            id,
            kind: kind.to_owned(),
        },
    )
    .await
}

/// Send a composed message. Threading headers are passed straight back from a `ComposeDraft`.
#[allow(clippy::too_many_arguments)]
pub async fn send_message(
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
    call(
        "send_message",
        &SendArgs {
            account_id,
            to,
            cc,
            subject,
            body,
            in_reply_to,
            references,
            attachments,
            markdown,
            draft_id,
        },
    )
    .await
}

/// Save (or update) a local draft (with its attachment file paths); returns its id so the composer
/// keeps editing the same row.
pub async fn save_draft(
    account_id: i64,
    draft_id: Option<i64>,
    draft: ComposeDraft,
    attachments: Vec<String>,
) -> Result<i64, String> {
    call(
        "save_draft",
        &SaveDraftArgs {
            account_id,
            draft_id,
            draft,
            attachments,
        },
    )
    .await
}

/// Every saved draft for an account, newest first.
pub async fn list_drafts(account_id: i64) -> Result<Vec<DraftSummary>, String> {
    call("list_drafts", &AccountArgs { account_id }).await
}

/// Load a draft back into a compose form + its attachment paths (`None` if it's gone).
pub async fn load_draft(id: i64) -> Result<Option<ResumedDraft>, String> {
    call("load_draft", &IdArgs { id }).await
}

/// Sync the provider's Drafts folder, so a draft started in webmail turns up here. `false` = the
/// provider keeps no Drafts folder, so the drafts live on this device.
pub async fn refresh_drafts(account_id: i64) -> Result<bool, String> {
    call("refresh_drafts", &AccountArgs { account_id }).await
}

/// Continue a draft that's in the provider's Drafts folder: its text plus its attachments, fetched
/// and written to temp files so the composer handles them like any other.
pub async fn resume_server_draft(id: i64) -> Result<ResumedDraft, String> {
    call("resume_server_draft", &IdArgs { id }).await
}

/// Take every server copy of this account's drafts back off the server — when "sync drafts" is
/// switched off. Local drafts are untouched.
pub async fn purge_server_drafts(account_id: i64) -> Result<(), String> {
    call("purge_server_drafts", &AccountArgs { account_id }).await
}

/// Delete a saved draft (idempotent).
pub async fn delete_draft(id: i64) -> Result<(), String> {
    call("delete_draft", &IdArgs { id }).await
}

/// Open a native file picker and return the chosen paths (empty if cancelled).
pub async fn pick_files() -> Result<Vec<String>, String> {
    call("pick_files", &NoArgs {}).await
}

/// Save an open message to disk as a `.eml`. `Ok(false)` if the user cancelled the save dialog.
pub async fn save_eml(id: i64) -> Result<bool, String> {
    call("save_eml", &IdArgs { id }).await
}

/// Open a `.eml` file into the account's local Saved folder; returns the new message id (or `None`
/// if cancelled) so the caller can switch to Saved and open it.
pub async fn open_eml_file(account_id: i64) -> Result<Option<i64>, String> {
    call("open_eml_file", &AccountArgs { account_id }).await
}

/// Save a message's `index`-th attachment to disk (fetched on demand). `Ok(false)` if cancelled.
pub async fn save_attachment(message_id: i64, index: usize) -> Result<bool, String> {
    call("save_attachment", &SaveAttachmentArgs { message_id, index }).await
}

/// Distinct past-sender addresses matching `prefix`, for To/Cc autocomplete. Empty for a blank prefix.
pub async fn suggest_addresses(account_id: i64, prefix: String) -> Result<Vec<String>, String> {
    call("suggest_addresses", &SuggestArgs { account_id, prefix }).await
}

/// Kick a refresh of `folder`: recent mail syncs first (this resolves when it's in), then older mail
/// backfills in the background, streaming `sync-progress` events (see [`on_sync_progress`]).
pub async fn refresh(account_id: i64, folder: &str) -> Result<(), String> {
    call(
        "refresh",
        &RefreshArgs {
            account_id,
            folder: folder.to_owned(),
        },
    )
    .await
}

/// Remove an account and its downloaded mail from this device (the server copy is untouched).
/// Returns whether the keychain password was cleared cleanly (the local mail is gone either way).
pub async fn remove_account(account_id: i64) -> Result<bool, String> {
    call("remove_account", &AccountArgs { account_id }).await
}

/// A persisted boolean preference, or `None` if never set.
pub async fn get_setting(key: &str) -> Result<Option<String>, String> {
    call(
        "get_setting",
        &KeyArg {
            key: key.to_owned(),
        },
    )
    .await
}

/// Save a free-text setting (quiet hours).
pub async fn set_setting(key: &str, value: &str) -> Result<(), String> {
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Args {
        key: String,
        value: String,
    }
    call(
        "set_setting",
        &Args {
            key: key.to_owned(),
            value: value.to_owned(),
        },
    )
    .await
}

/// A boolean setting (`None` = never set; the caller supplies the default).
pub async fn get_bool_setting(key: &str) -> Result<Option<bool>, String> {
    call(
        "get_bool_setting",
        &KeyArg {
            key: key.to_owned(),
        },
    )
    .await
}

/// Persist a boolean preference.
pub async fn set_bool_setting(key: &str, value: bool) -> Result<(), String> {
    call(
        "set_bool_setting",
        &KeyBoolArg {
            key: key.to_owned(),
            value,
        },
    )
    .await
}

/// An account's signature (empty if unset).
pub async fn get_signature(account_id: i64) -> Result<String, String> {
    call("get_signature", &AccountArgs { account_id }).await
}

/// Persist an account's signature.
pub async fn set_signature(account_id: i64, signature: &str) -> Result<(), String> {
    call(
        "set_signature",
        &SigArg {
            account_id,
            signature: signature.to_owned(),
        },
    )
    .await
}

/// Dev/test seam — see `geleit-app::ipc::dev_open_message`. Always `None` in a release build.
pub async fn dev_open_message() -> Result<Option<i64>, String> {
    call("dev_open_message", &NoArgs {}).await
}

/// Dev/test seam — see `geleit-app::ipc::dev_load_images`. Always `false` in a release build.
pub async fn dev_load_images() -> Result<bool, String> {
    call("dev_load_images", &NoArgs {}).await
}

/// Subscribe to backend `sync-progress` events (S9.4). `cb` receives the running batch count, or
/// `-1` when the background backfill has finished. No-op on the host target.
#[cfg(target_arch = "wasm32")]
pub fn on_sync_progress(cb: impl Fn(i64) + 'static) {
    let closure = wasm_bindgen::closure::Closure::<dyn Fn(wasm_bindgen::JsValue)>::new(
        move |v: wasm_bindgen::JsValue| {
            if let Some(n) = v.as_f64() {
                cb(n as i64);
            }
        },
    );
    geleit_on_sync_progress(closure.as_ref());
    closure.forget(); // lives for the app's lifetime — the subscription never unsubscribes
}
#[cfg(not(target_arch = "wasm32"))]
pub fn on_sync_progress(_cb: impl Fn(i64) + 'static) {}

/// New mail arrived on its own (the background scheduler found it, NOTIF-1). `cb` gets how many
/// messages are worth announcing. Subscribed once, for the app's lifetime.
#[cfg(target_arch = "wasm32")]
pub fn on_mail_arrived(cb: impl Fn(i64) + 'static) {
    let closure = wasm_bindgen::closure::Closure::<dyn Fn(wasm_bindgen::JsValue)>::new(
        move |v: wasm_bindgen::JsValue| {
            if let Some(n) = v.as_f64() {
                cb(n as i64);
            }
        },
    );
    geleit_on_mail_arrived(closure.as_ref());
    closure.forget();
}
#[cfg(not(target_arch = "wasm32"))]
pub fn on_mail_arrived(_cb: impl Fn(i64) + 'static) {}

/// Dev/test seam — see `geleit-app::ipc::dev_compose`. Always `None` in a release build.
pub async fn dev_compose() -> Result<Option<String>, String> {
    call("dev_compose", &NoArgs {}).await
}

/// Dev/test seam — see `geleit-app::ipc::dev_unified`. Always `false` in a release build.
pub async fn dev_unified() -> Result<bool, String> {
    call("dev_unified", &NoArgs {}).await
}

/// Dev/test seam — see `geleit-app::ipc::dev_settings`. Always `None` in a release build.
pub async fn dev_settings() -> Result<Option<String>, String> {
    call("dev_settings", &NoArgs {}).await
}

/// Dev/test seam — see `geleit-app::ipc::dev_search`. Always `None` in a release build.
pub async fn dev_search() -> Result<Option<String>, String> {
    call("dev_search", &NoArgs {}).await
}

/// Dev/test seam — see `geleit-app::ipc::dev_trash`. Always `None` in a release build.
pub async fn dev_trash() -> Result<Option<String>, String> {
    call("dev_trash", &NoArgs {}).await
}

/// Dev/test seam — see `geleit-app::ipc::dev_compose_to`. Always `None` in a release build.
pub async fn dev_compose_to() -> Result<Option<String>, String> {
    call("dev_compose_to", &NoArgs {}).await
}

/// Dev/test seam — see `geleit-app::ipc::dev_drafts`. Always `false` in a release build.
pub async fn dev_drafts() -> Result<bool, String> {
    call("dev_drafts", &NoArgs {}).await
}

/// Dev/test seam — see `geleit-app::ipc::dev_resume`. Always `false` in a release build.
pub async fn dev_resume() -> Result<bool, String> {
    call("dev_resume", &NoArgs {}).await
}

/// Dev/test seam — see `geleit-app::ipc::dev_select`. Always `None` in a release build.
pub async fn dev_select() -> Result<Option<String>, String> {
    call("dev_select", &NoArgs {}).await
}

/// Dev/test seam — see `geleit-app::ipc::dev_folder`. Always `None` in a release build.
pub async fn dev_folder() -> Result<Option<String>, String> {
    call("dev_folder", &NoArgs {}).await
}

/// Dev/test seam — see `geleit-app::ipc::dev_setup`. Always `false` in a release build.
pub async fn dev_setup() -> Result<bool, String> {
    call("dev_setup", &NoArgs {}).await
}
