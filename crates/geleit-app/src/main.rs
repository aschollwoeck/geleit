//! `geleit-app` — the Slint shell (S1.7). Renders the local store's folders and a virtualized
//! message list in the "Soft daylight" design (`design.md`). Reads the store only — no network on
//! the UI path (constitution P1); sync is the engine's job, wired in later (S1.9).

mod refresh;
mod viewmodel;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use geleit_engine::imap;
use geleit_platform::os_secret::OsSecretStore;
use geleit_store::Store;
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};

// TODO (design polish, later slices): real line-icon for the attachment marker (design.md §7,
// currently "[paperclip]"), bundle the Hanken Grotesk font (§3), per-account avatar initial + a
// folder hover state, and the selected-message guide edge (arrives with selection in S1.8).
slint::slint! {
    import { ListView, ScrollView, LineEdit } from "std-widgets.slint";

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
        // add-account form
        in property <bool> needs-setup;
        in-out property <string> f-email;
        in-out property <string> f-name;
        in-out property <string> f-host;
        in-out property <string> f-port;
        in-out property <string> f-user;
        in-out property <string> f-pass;
        in property <bool> setup-busy;
        in property <string> setup-error;
        callback folder-selected(int);
        callback message-selected(MessageItem);
        callback mark-unread(int);
        callback refresh();
        callback connect();
        callback reload();

        preferred-width: 1100px;
        preferred-height: 720px;
        title: "GeleitMail";
        background: Palette.bg;
        default-font-size: 15px;

        // ---- MAIN VIEW (an account exists) ----
        if !root.needs-setup: HorizontalLayout {
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

        // ---- ADD-ACCOUNT FORM (no account yet, or reconnect) ----
        if root.needs-setup: Rectangle {
            background: Palette.bg;
            VerticalLayout {
                alignment: center;
                HorizontalLayout {
                    alignment: center;
                    Rectangle {
                        width: 440px;
                        background: Palette.surface;
                        border-radius: 14px;
                        Rectangle { x: 0; width: 3px; background: Palette.accent; } // guide edge
                        VerticalLayout {
                            padding: 28px;
                            spacing: 10px;
                            Text {
                                text: "Add your account";
                                color: Palette.text;
                                font-size: 20px;
                                font-weight: 600;
                            }
                            Text {
                                text: "Connect over IMAP. Your details stay on this device.";
                                color: Palette.muted;
                                font-size: 13px;
                                wrap: word-wrap;
                            }
                            Text { text: "Email"; color: Palette.muted; font-size: 12px; }
                            LineEdit { placeholder-text: "you@example.com"; text <=> root.f-email; }
                            Text { text: "Display name (optional)"; color: Palette.muted; font-size: 12px; }
                            LineEdit { placeholder-text: "Your name"; text <=> root.f-name; }
                            Text { text: "IMAP server"; color: Palette.muted; font-size: 12px; }
                            LineEdit { placeholder-text: "imap.example.com"; text <=> root.f-host; }
                            Text { text: "Port"; color: Palette.muted; font-size: 12px; }
                            LineEdit { placeholder-text: "993"; text <=> root.f-port; }
                            Text { text: "Username"; color: Palette.muted; font-size: 12px; }
                            LineEdit { placeholder-text: "usually your email"; text <=> root.f-user; }
                            Text { text: "Password"; color: Palette.muted; font-size: 12px; }
                            LineEdit { input-type: password; text <=> root.f-pass; }
                            if root.setup-error != "": Text {
                                text: root.setup-error;
                                color: Palette.danger-strong;
                                font-size: 13px;
                                wrap: word-wrap;
                            }
                            Rectangle {
                                height: 40px;
                                border-radius: 8px;
                                background: Palette.accent-strong;
                                opacity: root.setup-busy ? 0.6 : 1.0;
                                HorizontalLayout {
                                    alignment: center;
                                    Text {
                                        text: root.setup-busy ? "Connecting…" : "Connect";
                                        color: white;
                                        font-size: 14px;
                                        font-weight: 600;
                                        vertical-alignment: center;
                                    }
                                }
                                TouchArea {
                                    enabled: !root.setup-busy;
                                    clicked => { root.connect(); }
                                }
                            }
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

/// Re-read account / folders / current-folder messages into the UI (UI thread only). Sets
/// `needs-setup` when there is no account, so the Add-account form shows.
fn reload_all(
    ui: &Main,
    store: &Store,
    folders_model: &VecModel<SharedString>,
    folder_ids: &RefCell<Vec<i64>>,
    messages: &VecModel<MessageItem>,
) {
    ui.set_status(SharedString::new()); // clear any stale main-view banner
    match store
        .list_accounts()
        .ok()
        .and_then(|a| a.into_iter().next())
    {
        Some(acc) => {
            ui.set_needs_setup(false);
            ui.set_account(acc.email.into());
            let folders = store.folders_for_account(acc.id).unwrap_or_default();
            folders_model.set_vec(
                folders
                    .iter()
                    .map(|f| SharedString::from(f.name.as_str()))
                    .collect::<Vec<_>>(),
            );
            *folder_ids.borrow_mut() = folders.iter().map(|f| f.id).collect();
            ui.set_selected_folder(0);
            ui.set_selected_message(0);
            let first = folder_ids.borrow().first().copied().unwrap_or(-1);
            messages.set_vec(load_messages(store, first));
        }
        None => {
            ui.set_needs_setup(true);
            ui.set_account(SharedString::new());
            folders_model.set_vec(Vec::new());
            folder_ids.borrow_mut().clear();
            messages.set_vec(Vec::new());
        }
    }
}

/// Post a list reload (current folder) to the UI thread from a worker, optionally setting `status`.
/// Reuses the `folder-selected` reload path (shared model + UI store connection).
fn post_reload(weak: &slint::Weak<Main>, status: Option<String>) {
    let weak = weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = weak.upgrade() {
            ui.set_status(status.map(SharedString::from).unwrap_or_default());
            ui.invoke_folder_selected(ui.get_selected_folder());
        }
    });
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let db = std::env::var("GELEIT_DB").unwrap_or_else(|_| "geleit.db".to_owned());
    let store = Rc::new(Store::open(&db)?);
    // Secret store backed by the OS keychain (S2.1). Send+Sync → shared across the UI + workers.
    let secrets = Arc::new(OsSecretStore::new());

    let ui = Main::new()?;
    let folders_model = Rc::new(VecModel::<SharedString>::default());
    let folder_ids = Rc::new(RefCell::new(Vec::<i64>::new()));
    let messages = Rc::new(VecModel::<MessageItem>::default());
    ui.set_folders(ModelRc::from(folders_model.clone()));
    ui.set_messages(ModelRc::from(messages.clone()));

    // full reload (also the initial load) — reused by setup success
    {
        let weak = ui.as_weak();
        let store = store.clone();
        let fm = folders_model.clone();
        let fids = folder_ids.clone();
        let msgs = messages.clone();
        ui.on_reload(move || {
            if let Some(ui) = weak.upgrade() {
                reload_all(&ui, &store, &fm, &fids, &msgs);
            }
        });
    }
    ui.invoke_reload();

    // folder click → load that folder's list; clear the open message
    {
        let weak = ui.as_weak();
        let store = store.clone();
        let fids = folder_ids.clone();
        let model = messages.clone();
        ui.on_folder_selected(move |idx| {
            let Some(ui) = weak.upgrade() else { return };
            let Some(fid) = fids.borrow().get(idx as usize).copied() else {
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
        let store = store.clone();
        let secrets = secrets.clone();
        let folders_model = folders_model.clone();
        ui.on_refresh(move || {
            let Some(ui) = weak.upgrade() else { return };
            if ui.get_refreshing() {
                return; // already in flight
            }
            let Some(account) = store
                .list_accounts()
                .ok()
                .and_then(|a| a.into_iter().next())
            else {
                ui.set_status("No account configured yet.".into());
                return;
            };
            let Some(settings) = store.imap_settings(account.id).ok().flatten() else {
                ui.set_status("This account isn't set up for syncing.".into());
                return;
            };
            // post-restart: no session password → re-show the form pre-filled to reconnect
            if !imap::has_password(&*secrets, &settings.username).unwrap_or(false) {
                ui.set_f_email(account.email.clone().into());
                ui.set_f_name(account.display_name.clone().unwrap_or_default().into());
                ui.set_f_host(settings.host.clone().into());
                ui.set_f_port(settings.port.to_string().into());
                ui.set_f_user(settings.username.clone().into());
                ui.set_f_pass(SharedString::new());
                ui.set_setup_error("Enter your password to reconnect.".into());
                ui.set_needs_setup(true);
                return;
            }
            let folder = folders_model
                .row_data(ui.get_selected_folder() as usize)
                .map(|s| s.to_string())
                .unwrap_or_else(|| "INBOX".to_owned());
            ui.set_refreshing(true);
            ui.set_status(SharedString::new());

            let weak = weak.clone();
            let db_path = db_path.clone();
            let secrets = secrets.clone();
            std::thread::spawn(move || {
                // Nothing !Send crosses: only `weak` + plain data + the Arc secrets (Send+Sync).
                // Phase 1: incremental sync (recent window) — fast.
                let sync = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    refresh::run_refresh(&db_path, &*secrets, &folder)
                }))
                .unwrap_or_else(|_| Err("Couldn't refresh — something went wrong.".to_owned()));
                if let Err(msg) = sync {
                    let w = weak.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = w.upgrade() {
                            ui.set_refreshing(false);
                            ui.set_status(msg.into());
                        }
                    });
                    return;
                }
                // Recent mail is in — show it now (button stays "Refreshing…" through backfill).
                post_reload(&weak, None);

                // Phase 2: backfill the rest in the background, streaming a calm progress line.
                let progress = weak.clone();
                let mut on_batch = move |n: usize| {
                    let p = progress.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = p.upgrade() {
                            ui.set_status(format!("Catching up… {n}").into());
                        }
                    });
                };
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    refresh::run_backfill(&db_path, &*secrets, &folder, 200, &mut on_batch)
                }));

                // Done: clear the status, show the full list, re-enable Refresh.
                let w = weak.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = w.upgrade() {
                        ui.set_refreshing(false);
                        ui.set_status(SharedString::new());
                        ui.invoke_folder_selected(ui.get_selected_folder());
                    }
                });
            });
        });
    }

    // Connect (add account / reconnect) → run setup on a worker thread, then reload.
    {
        let weak = ui.as_weak();
        let db_path = db.clone();
        let secrets = secrets.clone();
        ui.on_connect(move || {
            let Some(ui) = weak.upgrade() else { return };
            if ui.get_setup_busy() {
                return;
            }
            let (email, settings) = match refresh::build_settings(
                &ui.get_f_email(),
                &ui.get_f_host(),
                &ui.get_f_port(),
                &ui.get_f_user(),
                false,
            ) {
                Ok(v) => v,
                Err(e) => {
                    ui.set_setup_error(e.into());
                    return;
                }
            };
            let password = ui.get_f_pass().to_string();
            if password.is_empty() {
                ui.set_setup_error("Enter your password.".into());
                return;
            }
            let display = ui.get_f_name().to_string();
            let display = (!display.trim().is_empty()).then_some(display);
            ui.set_setup_busy(true);
            ui.set_setup_error(SharedString::new());

            let weak = weak.clone();
            let db_path = db_path.clone();
            let secrets = secrets.clone();
            std::thread::spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    refresh::run_setup(
                        &db_path,
                        &*secrets,
                        &email,
                        display.as_deref(),
                        settings,
                        &password,
                    )
                }))
                .unwrap_or_else(|_| Err("Couldn't connect — something went wrong.".to_owned()));
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(ui) = weak.upgrade() else { return };
                    ui.set_setup_busy(false);
                    match result {
                        Ok(()) => {
                            ui.set_f_pass(SharedString::new());
                            ui.set_setup_error(SharedString::new());
                            ui.invoke_reload(); // show the mail
                        }
                        Err(msg) => ui.set_setup_error(msg.into()),
                    }
                });
            });
        });
    }

    ui.run()?;
    Ok(())
}
