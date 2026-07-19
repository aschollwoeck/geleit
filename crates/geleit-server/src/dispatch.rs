//! The web host's analogue of Tauri's `generate_handler!`: one function that routes a command name +
//! JSON args to the matching [`geleit_host::commands`] function. The argument structs mirror the
//! frontend's camelCase payloads in `geleit-ui::api` exactly.
//!
//! Coverage is deliberately explicit — every `Unknown command` is a command not yet wired here, not a
//! silent gap. Slice 1 wires the full interactive surface; the auto-updater is stubbed (a self-hosted
//! server is updated by its operator, not from a browser tab).
use geleit_host::commands as c;
use geleit_host::dto::ComposeDraft;
use geleit_host::{AppState, Shell};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;

fn de<T: DeserializeOwned>(v: Value) -> Result<T, String> {
    serde_json::from_value(v).map_err(|_| "The mailbox got an unexpected request.".to_owned())
}

/// Run one command. `Ok(json)` is the command's reply (unit → JSON `null`); `Err(msg)` is its calm
/// error string, which the HTTP layer returns as a non-2xx body for the shim to throw.
#[allow(clippy::too_many_lines)]
pub async fn dispatch(
    state: &AppState,
    shell: &Arc<dyn Shell>,
    cmd: &str,
    args: Value,
) -> Result<Value, String> {
    macro_rules! j {
        ($e:expr) => {
            serde_json::to_value($e).map_err(|_| "Couldn't encode the reply.".to_owned())
        };
    }
    match cmd {
        // --- reads -------------------------------------------------------------------------------
        "list_accounts" => j!(c::list_accounts(state).await?),
        "list_folders" => {
            let a: AccountArgs = de(args)?;
            j!(c::list_folders(state, a.account_id).await?)
        }
        "list_messages" => {
            let a: FolderArgs = de(args)?;
            j!(c::list_messages(state, a.folder_id, a.limit).await?)
        }
        "list_all_messages" => {
            let a: LimitArgs = de(args)?;
            j!(c::list_all_messages(state, a.limit).await?)
        }
        "open_message" => {
            let a: OpenArgs = de(args)?;
            j!(c::open_message(state, a.id, a.mark_read).await?)
        }
        "search" => {
            let a: SearchArgs = de(args)?;
            j!(c::search(state, a.account_id, a.query).await?)
        }
        "search_all" => {
            let a: QueryArg = de(args)?;
            j!(c::search_all(state, a.query).await?)
        }
        "suggest_addresses" => {
            let a: SuggestArgs = de(args)?;
            j!(c::suggest_addresses(state, a.account_id, a.prefix).await?)
        }
        "theme" => j!(c::theme(state).await?),
        "get_setting" => {
            let a: KeyArg = de(args)?;
            j!(c::get_setting(state, a.key).await?)
        }
        "get_bool_setting" => {
            let a: KeyArg = de(args)?;
            j!(c::get_bool_setting(state, a.key).await?)
        }
        "get_signature" => {
            let a: AccountArgs = de(args)?;
            j!(c::get_signature(state, a.account_id).await?)
        }
        "outbox_status" => j!(c::outbox_status(state).await?),
        "list_outbox" => j!(c::list_outbox(state).await?),
        "list_drafts" => {
            let a: AccountArgs = de(args)?;
            j!(c::list_drafts(state, a.account_id).await?)
        }
        "list_snoozed" => {
            let a: AccountArgs = de(args)?;
            j!(c::list_snoozed(state, a.account_id).await?)
        }
        "list_rules" => {
            let a: AccountArgs = de(args)?;
            j!(c::list_rules(state, a.account_id).await?)
        }
        "snooze_presets" => j!(c::snooze_presets().await?),
        "app_version" => j!(c::app_version()),

        // --- triage / flags ----------------------------------------------------------------------
        "set_star" => {
            let a: StarArgs = de(args)?;
            j!(c::set_star(state, a.id, a.on).await?)
        }
        "set_read" => {
            let a: IdArgs = de(args)?;
            j!(c::set_read(state, a.id).await?)
        }
        "set_unread" => {
            let a: IdArgs = de(args)?;
            j!(c::set_unread(state, a.id).await?)
        }
        "move_to_role" => {
            let a: RoleArgs = de(args)?;
            j!(c::move_to_role(state, a.id, a.role).await?)
        }
        "move_to_folder" => {
            let a: FolderMoveArgs = de(args)?;
            j!(c::move_to_folder(state, a.id, a.folder).await?)
        }
        "empty_trash" => {
            let a: AccountArgs = de(args)?;
            j!(c::empty_trash(state, a.account_id).await?)
        }
        "delete_forever" => {
            let a: IdArgs = de(args)?;
            j!(c::delete_forever(state, a.id).await?)
        }

        // --- folders -----------------------------------------------------------------------------
        "create_folder" => {
            let a: CreateFolderArgs = de(args)?;
            j!(c::create_folder(state, a.account_id, a.name).await?)
        }
        "rename_folder" => {
            let a: RenameFolderArgs = de(args)?;
            j!(c::rename_folder(state, a.account_id, a.from, a.to).await?)
        }
        "delete_folder" => {
            let a: DeleteFolderArgs = de(args)?;
            j!(c::delete_folder(state, a.account_id, a.folder_id, a.name).await?)
        }

        // --- compose / drafts / outbox -----------------------------------------------------------
        "compose_draft" => {
            let a: ComposeDraftArgs = de(args)?;
            j!(c::compose_draft(state, a.id, a.kind).await?)
        }
        "send_message" => {
            let a: SendArgs = de(args)?;
            j!(c::send_message(
                state,
                a.account_id,
                a.to,
                a.cc,
                a.subject,
                a.body,
                a.in_reply_to,
                a.references,
                a.attachments,
                a.markdown,
                a.draft_id,
                a.outbox_edit_id,
            )
            .await?)
        }
        "save_draft" => {
            let a: SaveDraftArgs = de(args)?;
            j!(c::save_draft(state, a.account_id, a.draft_id, a.draft, a.attachments).await?)
        }
        "load_draft" => {
            let a: IdArgs = de(args)?;
            j!(c::load_draft(state, a.id).await?)
        }
        "delete_draft" => {
            let a: IdArgs = de(args)?;
            j!(c::delete_draft(state, a.id).await?)
        }
        "refresh_drafts" => {
            let a: AccountArgs = de(args)?;
            j!(c::refresh_drafts(state, a.account_id).await?)
        }
        "resume_server_draft" => {
            let a: IdArgs = de(args)?;
            j!(c::resume_server_draft(state, a.id).await?)
        }
        "purge_server_drafts" => {
            let a: AccountArgs = de(args)?;
            j!(c::purge_server_drafts(state, a.account_id).await?)
        }
        "retry_outbox" => {
            let a: IdArgs = de(args)?;
            j!(c::retry_outbox(state, a.id).await?)
        }
        "discard_outbox" => {
            let a: IdArgs = de(args)?;
            j!(c::discard_outbox(state, a.id).await?)
        }
        "edit_outbox" => {
            let a: IdArgs = de(args)?;
            j!(c::edit_outbox(state, a.id).await?)
        }

        // --- rules -------------------------------------------------------------------------------
        "add_rule" => {
            let a: AddRuleArgs = de(args)?;
            j!(c::add_rule(
                state,
                a.account_id,
                a.field,
                a.pattern,
                a.target_folder,
                a.mark_read,
                a.star,
            )
            .await?)
        }
        "delete_rule" => {
            let a: IdArgs = de(args)?;
            j!(c::delete_rule(state, a.id).await?)
        }
        "move_rule" => {
            let a: MoveRuleArgs = de(args)?;
            j!(c::move_rule(state, a.id, a.up).await?)
        }

        // --- settings / signature / theme --------------------------------------------------------
        "set_theme" => {
            let a: ThemeArg = de(args)?;
            j!(c::set_theme(state, a.theme).await?)
        }
        "set_setting" => {
            let a: KeyValueArgs = de(args)?;
            j!(c::set_setting(state, a.key, a.value).await?)
        }
        "set_bool_setting" => {
            let a: KeyBoolArgs = de(args)?;
            j!(c::set_bool_setting(state, a.key, a.value).await?)
        }
        "set_signature" => {
            let a: SigArgs = de(args)?;
            j!(c::set_signature(state, a.account_id, a.signature).await?)
        }

        // --- account lifecycle -------------------------------------------------------------------
        "add_account" => {
            let a: AccountFormArgs = de(args)?;
            // The host core adds + first-syncs the account; instant-IDLE is the desktop shell's
            // side-effect (the server's background workers cover it in a later slice).
            j!(c::add_account(
                state,
                a.email,
                a.display_name,
                a.imap_host,
                a.imap_port,
                a.username,
                a.password,
                a.smtp_host,
                a.smtp_port,
                a.smtp_starttls,
                a.signature,
                a.allow_invalid_certs,
            )
            .await?)
        }
        "remove_account" => {
            let a: AccountArgs = de(args)?;
            j!(c::remove_account(state, a.account_id).await?)
        }

        // --- native file I/O (zenity/kdialog — pops on the server's desktop; localhost-only) ------
        "pick_files" => j!(c::pick_files().await?),
        "save_eml" => {
            let a: IdArgs = de(args)?;
            j!(c::save_eml(state, a.id).await?)
        }
        "save_attachment" => {
            let a: SaveAttachmentArgs = de(args)?;
            j!(c::save_attachment(state, a.message_id, a.index).await?)
        }
        "export_folder" => {
            let a: ExportArgs = de(args)?;
            j!(c::export_folder(state, a.folder_id, a.folder_name).await?)
        }
        "export_account" => {
            let a: AccountArgs = de(args)?;
            j!(c::export_account(state, a.account_id).await?)
        }
        "open_eml_file" => {
            let a: AccountArgs = de(args)?;
            j!(c::open_eml_file(state, a.account_id).await?)
        }

        // --- commands that emit / set the badge --------------------------------------------------
        "update_badge" => j!(c::update_badge(shell.as_ref(), state).await?),
        "snooze_messages" => {
            let a: SnoozeArgs = de(args)?;
            j!(c::snooze_messages(shell.as_ref(), state, a.ids, a.until).await?)
        }
        "unsnooze_message" => {
            let a: IdArgs = de(args)?;
            j!(c::unsnooze_message(shell.as_ref(), state, a.id).await?)
        }
        "run_rules_now" => {
            let a: AccountArgs = de(args)?;
            j!(c::run_rules_now(shell.as_ref(), state, a.account_id).await?)
        }
        "refresh" => {
            let a: RefreshArgs = de(args)?;
            j!(c::refresh(shell.clone(), state, a.account_id, a.folder).await?)
        }

        // --- auto-updater: not applicable to a self-hosted server --------------------------------
        "check_update" => Ok(Value::Null), // null = up to date
        "install_update" => Err("Updates are managed by the server operator.".to_owned()),

        // --- dev seams (release UI never calls these) --------------------------------------------
        other => Err(format!("Unknown command: {other}")),
    }
}

// --- argument structs (mirror geleit-ui::api's camelCase payloads) -------------------------------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AccountArgs {
    account_id: i64,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FolderArgs {
    folder_id: i64,
    limit: i64,
}
#[derive(Deserialize)]
struct LimitArgs {
    limit: i64,
}
#[derive(Deserialize)]
struct QueryArg {
    query: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OpenArgs {
    id: i64,
    mark_read: bool,
}
#[derive(Deserialize)]
struct IdArgs {
    id: i64,
}
#[derive(Deserialize)]
struct StarArgs {
    id: i64,
    on: bool,
}
#[derive(Deserialize)]
struct RoleArgs {
    id: i64,
    role: String,
}
#[derive(Deserialize)]
struct FolderMoveArgs {
    id: i64,
    folder: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateFolderArgs {
    account_id: i64,
    name: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RenameFolderArgs {
    account_id: i64,
    from: String,
    to: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeleteFolderArgs {
    account_id: i64,
    folder_id: i64,
    name: String,
}
#[derive(Deserialize)]
struct ComposeDraftArgs {
    id: i64,
    kind: String,
}
#[derive(Deserialize)]
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
    outbox_edit_id: Option<i64>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SaveDraftArgs {
    account_id: i64,
    draft_id: Option<i64>,
    draft: ComposeDraft,
    attachments: Vec<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SuggestArgs {
    account_id: i64,
    prefix: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExportArgs {
    folder_id: i64,
    folder_name: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SaveAttachmentArgs {
    message_id: i64,
    index: usize,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SearchArgs {
    account_id: i64,
    query: String,
}
#[derive(Deserialize)]
struct ThemeArg {
    theme: String,
}
#[derive(Deserialize)]
struct KeyArg {
    key: String,
}
#[derive(Deserialize)]
struct KeyValueArgs {
    key: String,
    value: String,
}
#[derive(Deserialize)]
struct KeyBoolArgs {
    key: String,
    value: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SigArgs {
    account_id: i64,
    signature: String,
}
#[derive(Deserialize)]
struct SnoozeArgs {
    ids: Vec<i64>,
    until: i64,
}
#[derive(Deserialize)]
struct MoveRuleArgs {
    id: i64,
    up: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AddRuleArgs {
    account_id: i64,
    field: String,
    pattern: String,
    target_folder: Option<String>,
    mark_read: bool,
    star: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RefreshArgs {
    account_id: i64,
    folder: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AccountFormArgs {
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
}
