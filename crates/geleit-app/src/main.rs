//! `geleit-app` — the Slint shell (S1.7). Renders the local store's folders and a virtualized
//! message list in the "Soft daylight" design (`design.md`). Reads the store only — no network on
//! the UI path (constitution P1); sync is the engine's job, wired in later (S1.9).

mod refresh;
mod viewmodel;

use std::rc::Rc;

use geleit_store::Store;
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};

// TODO (design polish, later slices): real line-icon for the attachment marker (design.md §7,
// currently "[paperclip]"), bundle the Hanken Grotesk font (§3), per-account avatar initial + a
// folder hover state, and the selected-message guide edge (arrives with selection in S1.8).
slint::slint! {
    import { ListView, ScrollView } from "std-widgets.slint";

    // Soft-daylight tokens from design.md.
    global Palette {
        out property <color> bg: #f5f7f8;
        out property <color> surface: #ffffff;
        out property <color> surface-reading: #fbfaf7;
        out property <color> text: #1f2a2e;
        out property <color> muted: #5e7177;
        out property <color> accent: #2e9e9b;
        out property <color> accent-strong: #1c7e7b;
        out property <color> accent-quiet: #e2f1f0;
        out property <color> danger-strong: #b3472e;
        out property <color> danger-quiet: #fbe9e4;
        out property <color> divider: #e3eaec;
    }

    struct MessageItem {
        id: int,
        sender: string,
        subject: string,
        snippet: string,
        date: string,
        unread: bool,
        attachment: bool,
    }

    export component Main inherits Window {
        in property <string> account;
        in property <[string]> folders;
        in property <int> selected-folder;
        in property <[MessageItem]> messages;
        in property <int> selected-message; // selected message id (0 = none)
        in property <string> r-subject;
        in property <string> r-sender;
        in property <string> r-date;
        in property <string> r-body;
        in property <bool> refreshing;
        in property <string> status; // non-empty = error to show
        callback folder-selected(int);
        callback message-selected(MessageItem);
        callback mark-unread(int);
        callback refresh();

        preferred-width: 1100px;
        preferred-height: 720px;
        title: "GeleitMail";
        background: Palette.bg;
        default-font-size: 15px;

        HorizontalLayout {
            // ---- LEFT RAIL ----
            Rectangle {
                width: 240px;
                background: Palette.bg;
                VerticalLayout {
                    padding: 16px;
                    spacing: 4px;
                    HorizontalLayout {
                        spacing: 10px;
                        Rectangle {
                            width: 30px;
                            height: 30px;
                            border-radius: 15px;
                            background: Palette.accent;
                            Text { text: "A"; color: white; font-weight: 600; }
                        }
                        Text {
                            text: root.account;
                            color: Palette.text;
                            font-weight: 600;
                            vertical-alignment: center;
                        }
                    }
                    Rectangle { height: 12px; }
                    for f[i] in root.folders: Rectangle {
                        height: 40px; // design.md §9: ≥40px hit target
                        border-radius: 10px;
                        background: i == root.selected-folder ? Palette.accent-quiet : transparent;
                        TouchArea { clicked => { root.folder-selected(i); } }
                        HorizontalLayout {
                            padding-left: 12px;
                            Text {
                                text: f;
                                color: Palette.text;
                                vertical-alignment: center;
                                font-weight: i == root.selected-folder ? 600 : 400;
                            }
                        }
                    }
                }
            }

            // ---- MESSAGE LIST ----
            Rectangle {
                width: 380px;
                background: Palette.surface;
                VerticalLayout {
                    Rectangle {
                        height: 52px;
                        background: Palette.surface;
                        HorizontalLayout {
                            padding: 16px;
                            spacing: 8px;
                            Text {
                                text: root.selected-folder < root.folders.length ? root.folders[root.selected-folder] : "";
                                color: Palette.text;
                                font-size: 18px;
                                font-weight: 600;
                                vertical-alignment: center;
                                horizontal-stretch: 1;
                            }
                            Rectangle {
                                width: 104px;
                                height: 32px;
                                y: 10px;
                                border-radius: 8px;
                                // accent-strong text needs surface, not accent-quiet (AA, design.md §4)
                                background: Palette.surface;
                                border-width: 1px;
                                border-color: Palette.accent;
                                opacity: root.refreshing ? 0.6 : 1.0;
                                HorizontalLayout {
                                    alignment: center;
                                    Text {
                                        text: root.refreshing ? "Refreshing…" : "Refresh";
                                        color: Palette.accent-strong;
                                        font-size: 13px;
                                        font-weight: 600;
                                        vertical-alignment: center;
                                    }
                                }
                                TouchArea {
                                    enabled: !root.refreshing;
                                    clicked => { root.refresh(); }
                                }
                            }
                        }
                    }
                    Rectangle { height: 1px; background: Palette.divider; }
                    if root.status != "": Rectangle {
                        height: 44px;
                        background: Palette.danger-quiet;
                        Rectangle { x: 0; width: 3px; background: Palette.danger-strong; } // guide edge
                        HorizontalLayout {
                            padding-left: 14px;
                            padding-right: 14px;
                            Text {
                                text: root.status;
                                color: Palette.text; // body text on tint (AA), per design.md §10
                                font-size: 13px;
                                vertical-alignment: center;
                                wrap: word-wrap;
                            }
                        }
                    }
                    ListView {
                        for m in root.messages: Rectangle {
                            height: 72px;
                            background: m.id == root.selected-message ? Palette.accent-quiet : Palette.surface;
                            // selection guide edge (design.md signature)
                            Rectangle {
                                x: 0;
                                width: 3px;
                                visible: m.id == root.selected-message;
                                background: Palette.accent;
                            }
                            HorizontalLayout {
                                padding: 12px;
                                spacing: 10px;
                                Rectangle {
                                    width: 10px;
                                    Rectangle {
                                        width: 8px;
                                        height: 8px;
                                        y: 6px;
                                        border-radius: 4px;
                                        background: m.unread ? Palette.accent : transparent;
                                    }
                                }
                                VerticalLayout {
                                    spacing: 2px;
                                    Text {
                                        text: m.sender;
                                        color: Palette.text;
                                        font-size: 14px; // design.md §3
                                        font-weight: m.unread ? 600 : 500;
                                        overflow: elide;
                                    }
                                    Text {
                                        text: m.subject;
                                        color: Palette.text;
                                        font-weight: m.unread ? 600 : 500;
                                        overflow: elide;
                                    }
                                    Text {
                                        text: m.snippet;
                                        color: Palette.muted;
                                        font-size: 13px;
                                        overflow: elide;
                                    }
                                }
                                VerticalLayout {
                                    alignment: start;
                                    Text {
                                        text: m.date;
                                        color: Palette.muted;
                                        font-size: 12px;
                                        horizontal-alignment: right;
                                    }
                                    Text {
                                        text: m.attachment ? "[paperclip]" : "";
                                        color: Palette.muted;
                                        font-size: 12px;
                                        horizontal-alignment: right;
                                    }
                                }
                            }
                            Rectangle {
                                y: parent.height - 1px;
                                height: 1px;
                                background: Palette.divider;
                            }
                            TouchArea { clicked => { root.message-selected(m); } }
                        }
                    }
                }
            }

            // ---- READING PANE ----
            Rectangle {
                background: Palette.surface-reading;
                Rectangle { x: 0; width: 3px; background: Palette.accent; } // guide edge
                if root.selected-message == 0: VerticalLayout {
                    padding: 28px;
                    Text { text: "Select a message to read it."; color: Palette.muted; }
                }
                if root.selected-message != 0: VerticalLayout {
                    padding: 28px;
                    spacing: 10px;
                    Text {
                        text: root.r-subject;
                        color: Palette.text;
                        font-size: 21px;
                        font-weight: 600;
                        wrap: word-wrap;
                    }
                    Text {
                        text: root.r-sender + "  ·  " + root.r-date;
                        color: Palette.muted;
                        font-size: 13px;
                    }
                    TouchArea {
                        height: 22px;
                        clicked => { root.mark-unread(root.selected-message); }
                        HorizontalLayout {
                            alignment: start;
                            Text { text: "Mark as unread"; color: Palette.accent-strong; font-size: 13px; }
                        }
                    }
                    ScrollView {
                        Text {
                            text: root.r-body;
                            color: Palette.text;
                            wrap: word-wrap;
                        }
                    }
                }
            }
        }
    }
}

fn load_messages(store: &Store, folder_id: i64) -> Vec<MessageItem> {
    store
        .messages_in_folder(folder_id, 1000)
        .unwrap_or_default()
        .iter()
        .map(|h| {
            let vm = viewmodel::message_vm(h);
            MessageItem {
                id: h.id as i32,
                sender: vm.sender.into(),
                subject: vm.subject.into(),
                snippet: vm.snippet.into(),
                date: vm.date.into(),
                unread: vm.unread,
                attachment: vm.attachment,
            }
        })
        .collect()
}

/// Flip one row's `unread` flag in place — preserves the list scroll position and avoids
/// re-querying the whole folder on every read toggle.
fn flip_unread(model: &VecModel<MessageItem>, id: i32, unread: bool) {
    for i in 0..model.row_count() {
        if let Some(mut row) = model.row_data(i) {
            if row.id == id {
                row.unread = unread;
                model.set_row_data(i, row);
                break;
            }
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let db = std::env::var("GELEIT_DB").unwrap_or_else(|_| "geleit.db".to_owned());
    let store = Store::open(&db)?;

    let account = store.list_accounts()?.into_iter().next();
    let (account_label, folder_names, folder_ids) = match &account {
        Some(a) => {
            let folders = store.folders_for_account(a.id)?;
            (
                a.email.clone(),
                folders
                    .iter()
                    .map(|f| SharedString::from(f.name.as_str()))
                    .collect::<Vec<_>>(),
                folders.iter().map(|f| f.id).collect::<Vec<_>>(),
            )
        }
        None => ("(no account)".to_owned(), Vec::new(), Vec::new()),
    };

    // plain-String folder names for the refresh worker (the SharedString model is UI-thread only)
    let folder_name_strs: Vec<String> = folder_names.iter().map(ToString::to_string).collect();

    let ui = Main::new()?;
    ui.set_account(account_label.into());
    ui.set_folders(ModelRc::new(VecModel::from(folder_names)));
    ui.set_selected_folder(0);

    let store = Rc::new(store);
    let folder_ids = Rc::new(folder_ids);

    let initial_folder = folder_ids.first().copied().unwrap_or(-1);
    let messages = Rc::new(VecModel::from(load_messages(&store, initial_folder)));
    ui.set_messages(ModelRc::from(messages.clone()));

    // folder click → load that folder's list; clear the open message
    {
        let weak = ui.as_weak();
        let store = store.clone();
        let fids = folder_ids.clone();
        let model = messages.clone();
        ui.on_folder_selected(move |idx| {
            let (Some(ui), Some(&fid)) = (weak.upgrade(), fids.get(idx as usize)) else {
                return;
            };
            ui.set_selected_folder(idx);
            ui.set_selected_message(0);
            model.set_vec(load_messages(&store, fid));
        });
    }

    // message click → open it in the reading pane and mark it read
    {
        let weak = ui.as_weak();
        let store = store.clone();
        let model = messages.clone();
        ui.on_message_selected(move |item| {
            let Some(ui) = weak.upgrade() else { return };
            ui.set_selected_message(item.id);
            ui.set_r_subject(item.subject.clone());
            ui.set_r_sender(item.sender.clone());
            ui.set_r_date(item.date.clone());
            let body = match store.body_for(item.id.into()) {
                Ok(b) => viewmodel::body_display(b.as_ref()),
                Err(_) => "(Could not load this message.)".to_owned(),
            };
            ui.set_r_body(body.into());
            if item.unread {
                let _ = store.set_seen(item.id.into(), true);
                flip_unread(&model, item.id, false); // in place — keeps scroll
            }
        });
    }

    // "Mark as unread" → flip read state locally and update the row in place
    {
        let store = store.clone();
        let model = messages.clone();
        ui.on_mark_unread(move |id| {
            let _ = store.set_seen(id.into(), false);
            flip_unread(&model, id, true);
        });
    }

    // Refresh → sync on a worker thread (P1: never block the UI), then reload on the UI thread.
    {
        let weak = ui.as_weak();
        let db_path = db.clone();
        let folder_names = folder_name_strs;
        ui.on_refresh(move || {
            let Some(ui) = weak.upgrade() else { return };
            if ui.get_refreshing() {
                return; // already in flight
            }
            let (config, password) = match refresh::config_from_env() {
                Ok(v) => v,
                Err(e) => {
                    ui.set_status(e.into());
                    return;
                }
            };
            // sync (and reload) the folder the user is looking at; default to INBOX
            let folder = folder_names
                .get(ui.get_selected_folder() as usize)
                .cloned()
                .unwrap_or_else(|| "INBOX".to_owned());
            ui.set_refreshing(true);
            ui.set_status(SharedString::new());

            let weak = weak.clone();
            let db_path = db_path.clone();
            std::thread::spawn(move || {
                // Nothing !Send crosses here: only `weak` (Send) + plain data; the result post too.
                // catch_unwind so a worker panic still clears `refreshing` (never a stuck button).
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    refresh::run_refresh(&db_path, config, &password, &folder)
                }))
                .unwrap_or_else(|_| Err("Couldn't refresh — something went wrong.".to_owned()));
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(ui) = weak.upgrade() else { return };
                    ui.set_refreshing(false);
                    match result {
                        Ok(()) => {
                            ui.set_status(SharedString::new());
                            // reuse the existing reload path (shared model + UI store connection)
                            ui.invoke_folder_selected(ui.get_selected_folder());
                        }
                        Err(msg) => ui.set_status(msg.into()),
                    }
                });
            });
        });
    }

    ui.run()?;
    Ok(())
}
