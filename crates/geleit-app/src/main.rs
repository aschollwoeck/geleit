//! `geleit-app` — the Slint shell (S1.7). Renders the local store's folders and a virtualized
//! message list in the "Soft daylight" design (`design.md`). Reads the store only — no network on
//! the UI path (constitution P1); sync is the engine's job, wired in later (S1.9).

mod refresh;
mod viewmodel;

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use geleit_engine::imap;
use geleit_platform::os_secret::OsSecretStore;
use geleit_store::Store;
use slint::winit_030::WinitWindowAccessor;
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};

/// Shared handle to the embedded HTML webview + whether it's currently shown.
#[derive(Default)]
struct HtmlView {
    webview: RefCell<Option<wry::WebView>>,
    visible: Cell<bool>,
}

/// The reading-pane body region (logical coords) the webview should cover — right of the rail
/// (240) + list (380), below the subject/sender header (~132).
fn body_rect(ui: &Main) -> wry::Rect {
    let scale = ui.window().scale_factor();
    let phys = ui.window().size();
    // leave room for the "remote content blocked" cue bar when it's shown
    let top = if ui.get_remote_blocked() {
        168.0
    } else {
        132.0
    };
    let w = (phys.width as f32 / scale) - 620.0;
    let h = (phys.height as f32 / scale) - top;
    wry::Rect {
        position: wry::dpi::LogicalPosition::new(620.0_f32, top).into(),
        size: wry::dpi::LogicalSize::new(w.max(0.0), h.max(0.0)).into(),
    }
}

/// Build the child webview **once, up front** (hidden, pre-painted with the reading-pane background),
/// so the first mail open is instant — no webkit init on the UI thread mid-click, and no black
/// flash. No-op if already built or embedding is unavailable (e.g. Wayland — `build_as_child` is
/// X11-only), in which case the plain-text pane is the fallback.
fn ensure_webview(ui: &Main, view: &HtmlView) {
    if view.webview.borrow().is_some() {
        return;
    }
    let rect = body_rect(ui);
    let built = ui.window().with_winit_window(|win| {
        wry::WebViewBuilder::new()
            // defense-in-depth (guidelines §13): sanitization already removes scripts, but disable
            // JS in the webview too so a sanitizer miss still can't execute.
            .with_javascript_disabled()
            // Open real links in the system browser; never let the pane navigate away from our CSP'd
            // document (that would drop the sandbox + load remote content). Our own content loads
            // (about:blank / data:) return true and render normally.
            .with_navigation_handler(|url: String| {
                if url.starts_with("http://")
                    || url.starts_with("https://")
                    || url.starts_with("mailto:")
                {
                    let _ = std::process::Command::new("xdg-open").arg(&url).spawn();
                    false // handled externally — don't navigate in-pane
                } else {
                    true
                }
            })
            .with_bounds(rect)
            .build_as_child(win)
    });
    if let Some(Ok(w)) = built {
        // pre-paint the calm page background so revealing it never flashes black
        let _ = w.load_html(&geleit_engine::safehtml::document("", false));
        let _ = w.set_visible(false);
        *view.webview.borrow_mut() = Some(w);
    }
}

/// Render `sanitized_html` in the embedded sandboxed webview over the reading-pane body.
fn show_html(ui: &Main, view: &HtmlView, sanitized_html: &str) {
    ensure_webview(ui, view);
    let rect = body_rect(ui);
    if let Some(w) = view.webview.borrow().as_ref() {
        let _ = w.load_html(sanitized_html);
        let _ = w.set_bounds(rect);
        let _ = w.set_visible(true);
        view.visible.set(true);
    }
}

/// Hide the embedded webview so the Slint reading pane (text / form) shows.
fn hide_html(view: &HtmlView) {
    if let Some(w) = view.webview.borrow().as_ref() {
        let _ = w.set_visible(false);
    }
    view.visible.set(false);
}

// TODO (design polish, later slices): real line-icon for the attachment marker (design.md §7,
// currently "[paperclip]"), bundle the Hanken Grotesk font (§3), per-account avatar initial + a
// folder hover state, and the selected-message guide edge (arrives with selection in S1.8).
slint::slint! {
    import { ListView, ScrollView } from "std-widgets.slint";

    // Soft-daylight tokens from design.md.
    // "Soft daylight" (light) + "Soft dusk" (dark). `dark` is set from the persisted setting (APP-3).
    export global Palette {
        in property <bool> dark;
        out property <color> bg: dark ? #11191c : #f5f7f8;
        out property <color> surface: dark ? #18242a : #ffffff;
        out property <color> surface-reading: dark ? #16211f : #fbfaf7;
        out property <color> text: dark ? #e6eef0 : #1f2a2e;
        out property <color> muted: dark ? #93a8ad : #5e7177;
        out property <color> accent: dark ? #36b3af : #2e9e9b;
        out property <color> accent-strong: dark ? #5fc7c4 : #1c7e7b;
        out property <color> accent-quiet: dark ? #1e3a39 : #e2f1f0;
        out property <color> danger-strong: dark ? #e08f7a : #b3472e;
        out property <color> danger-quiet: dark ? #3a221d : #fbe9e4;
        out property <color> divider: dark ? #25343a : #e3eaec;
    }

    struct MessageItem {
        id: int,
        sender: string,
        subject: string,
        snippet: string,
        date: string,
        unread: bool,
        attachment: bool,
        thread-count: int, // messages in this conversation (1 = not threaded)
        starred: bool,
        selected: bool, // multi-select for bulk actions (ORG-7)
        account: int, // owning account (set for cross-account search hits; 0 = current view)
    }

    struct AccountItem {
        id: int,
        email: string,
    }

    struct DraftItem {
        id: int,
        subject: string,
        recipients: string,
        date: string,
    }

    // A text input styled to the Soft-daylight palette: always visible (surface fill + divider
    // border), accent border on focus, dark text, muted placeholder. Replaces std-widgets LineEdit/
    // TextEdit, whose default style is invisible against our background.
    component Field inherits Rectangle {
        in-out property <string> text <=> input.text;
        in property <string> placeholder;
        in property <bool> password;
        in property <bool> multiline;
        callback edited;
        min-height: 38px;
        border-radius: 8px;
        background: Palette.surface;
        border-width: 1px;
        border-color: input.has-focus ? Palette.accent : Palette.divider;
        clip: true;
        forward-focus: input;
        HorizontalLayout {
            padding: 9px;
            input := TextInput {
                color: Palette.text;
                font-size: 14px;
                single-line: !root.multiline;
                wrap: root.multiline ? word-wrap : no-wrap;
                input-type: root.password ? InputType.password : InputType.text;
                vertical-alignment: root.multiline ? top : center;
                edited => { root.edited(); }
            }
        }
        if root.text == "": Text {
            text: root.placeholder;
            color: Palette.muted;
            font-size: 14px;
            x: 9px;
            y: root.multiline ? 9px : (parent.height - self.height) / 2;
        }
    }

    export component Main inherits Window {
        in property <string> account;
        in property <[AccountItem]> accounts; // all accounts, for the switcher (MULTI-1)
        in property <int> current-account; // id of the account in view
        in property <[string]> folders;
        in property <int> selected-folder;
        in property <[MessageItem]> messages;
        in property <int> selected-message; // selected message id (0 = none)
        in-out property <int> nav-index: -1; // keyboard-focused list row (READ-9); -1 = none
        in property <string> r-subject;
        in property <string> r-sender;
        in property <string> r-date;
        in property <string> r-body;
        in property <bool> r-starred; // the open message is starred (ORG-4)
        in property <[string]> r-attachments;
        in property <bool> picking-folder; // "Move to…" folder picker open (ORG-3)
        in property <[string]> move-folders;
        in property <bool> viewing-trash; // current folder is Trash → show Empty Trash (ORG-2)
        in property <bool> viewing-junk; // current folder is Junk → action reads "Not spam" (ORG-5)
        // folder management (ORG-6)
        in property <bool> managing-folders;
        in-out property <string> mf-name;
        in property <[string]> manage-folders;
        in property <int> selected-count; // # messages multi-selected (ORG-7); 0 = bar hidden
        // search (SEARCH-1/2/3)
        in-out property <string> search-query;
        in property <bool> searching; // showing search results instead of a folder
        in property <int> search-count;
        in-out property <bool> search-all; // search across all accounts (SEARCH-5)
        in property <bool> refreshing;
        in property <string> status; // non-empty = error to show (danger banner)
        in property <string> sync-status; // non-empty = calm sync-progress line
        in property <bool> remote-blocked; // HTML message had remote content stripped (PRIV-3)
        // compose (M4 send)
        in property <bool> composing;
        in-out property <string> c-to;
        in-out property <string> c-cc;
        in-out property <string> c-subject;
        in-out property <string> c-body;
        in property <bool> sending;
        in property <string> compose-status; // non-empty = send error
        in property <[string]> c-attachments; // "name · size" per attached file (SEND-4)
        in-out property <string> c-attach-path;
        in property <[string]> c-suggestions; // address autocomplete for To (SEND-9)
        in property <[string]> c-cc-suggestions; // …and for Cc
        in-out property <bool> c-markdown; // send a Markdown-rendered HTML alternative (SEND-6)
        // drafts (SEND-5)
        in property <[DraftItem]> drafts;
        in property <bool> viewing-drafts;
        // add-account form
        in property <bool> needs-setup;
        in property <bool> adding-account; // showing the form to add ANOTHER account (MULTI-1)
        in-out property <string> f-email;
        in-out property <string> f-name;
        in-out property <string> f-host;
        in-out property <string> f-port;
        in-out property <string> f-user;
        in-out property <string> f-pass;
        in-out property <string> f-smtp-host;
        in-out property <string> f-smtp-port;
        in-out property <bool> f-smtp-starttls;
        in-out property <string> f-signature;
        in property <bool> setup-busy;
        in property <string> setup-error;
        private property <bool> confirm-remove;
        callback folder-selected(int);
        callback message-selected(MessageItem);
        callback mark-unread(int);
        callback toggle-star(int);
        callback archive-message(int);
        callback trash-message(int);
        callback open-move(int);
        callback move-to(string);
        callback close-move();
        callback empty-trash();
        callback toggle-junk(int);
        callback open-manage-folders();
        callback close-manage-folders();
        callback create-folder();
        callback rename-folder(string);
        callback delete-folder(string);
        callback toggle-select(int);
        callback clear-selection();
        callback bulk-archive();
        callback bulk-trash();
        callback bulk-star();
        callback bulk-mark-read();
        callback search-edited();
        callback clear-search();
        callback toggle-search-all();
        callback refresh();
        callback connect();
        callback reload();
        callback remove-account();
        callback switch-account(int);
        callback add-account();
        callback cancel-add-account();
        // keyboard navigation (READ-9 / APP-6)
        callback nav-next();
        callback nav-prev();
        callback nav-escape();
        // settings (APP-3/4)
        in property <bool> showing-settings;
        callback open-settings();
        callback close-settings();
        callback toggle-theme();
        callback load-remote();
        callback compose();
        callback send-message();
        callback cancel-compose();
        callback reply();
        callback reply-all();
        callback forward();
        callback open-drafts();
        callback resume-draft(int);
        callback save-draft();
        callback close-drafts();
        callback attach-file();
        callback remove-attachment(int);
        callback to-edited();
        callback pick-suggestion(string);
        callback cc-edited();
        callback pick-cc-suggestion(string);
        callback browse-file();

        preferred-width: 1100px;
        preferred-height: 720px;
        title: "GeleitMail";
        background: Palette.bg;
        default-font-size: 15px;
        forward-focus: keyscope;

        // Global keyboard shortcuts (READ-9 / APP-6). Sits BEHIND the UI and stays focused —
        // TouchAreas don't take keyboard focus, so shortcuts work everywhere except while typing in a
        // field (which grabs focus, correctly pausing them). Empty + full-size → no layout impact.
        keyscope := FocusScope {
            width: 100%;
            height: 100%;
            key-pressed(event) => {
                // don't hijack keys while composing / in an overlay (let fields type freely)
                if root.composing || root.viewing-drafts || root.picking-folder
                    || root.managing-folders || root.needs-setup || root.adding-account
                    || root.showing-settings {
                    if event.text == Key.Escape {
                        root.nav-escape();
                        return accept;
                    }
                    return reject;
                }
                if event.text == "j" || event.text == Key.DownArrow {
                    root.nav-next();
                    return accept;
                }
                if event.text == "k" || event.text == Key.UpArrow {
                    root.nav-prev();
                    return accept;
                }
                if event.text == "c" {
                    root.compose();
                    return accept;
                }
                if event.text == "r" && root.selected-message != 0 {
                    root.reply();
                    return accept;
                }
                if event.text == Key.Escape {
                    root.nav-escape();
                    return accept;
                }
                reject
            }
        }

        // ---- MAIN VIEW (an account exists) ----
        if !root.needs-setup && !root.adding-account: HorizontalLayout {
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
                            Text { text: "✉"; color: white; font-weight: 600; }
                        }
                        Text {
                            text: root.account;
                            color: Palette.text;
                            font-weight: 600;
                            vertical-alignment: center;
                            overflow: elide;
                        }
                    }
                    // account switcher (MULTI-1): other accounts to switch to + add another
                    for a in root.accounts: Rectangle {
                        height: a.id != root.current-account ? 28px : 0px;
                        visible: a.id != root.current-account;
                        border-radius: 6px;
                        background: sw.has-hover ? Palette.accent-quiet : transparent;
                        sw := TouchArea { clicked => { root.switch-account(a.id); } }
                        Text {
                            text: a.email;
                            x: 40px;
                            color: Palette.muted;
                            font-size: 12px;
                            vertical-alignment: center;
                            overflow: elide;
                        }
                    }
                    Rectangle {
                        height: 28px;
                        TouchArea { clicked => { root.add-account(); } }
                        Text {
                            text: "+ Add account";
                            x: 40px;
                            color: Palette.accent-strong;
                            font-size: 12px;
                            font-weight: 600;
                            vertical-alignment: center;
                        }
                    }
                    Rectangle { height: 12px; }
                    // Compose a new message (M4)
                    Rectangle {
                        height: 40px;
                        border-radius: 10px;
                        background: Palette.accent-strong;
                        HorizontalLayout {
                            alignment: center;
                            spacing: 6px;
                            Text {
                                text: "✏  New message";
                                color: white;
                                font-weight: 600;
                                vertical-alignment: center;
                            }
                        }
                        TouchArea { clicked => { root.compose(); } }
                    }
                    Rectangle { height: 6px; }
                    Rectangle {
                        height: 32px;
                        border-radius: 8px;
                        HorizontalLayout {
                            alignment: center;
                            Text {
                                text: "Drafts";
                                color: Palette.accent-strong;
                                font-size: 13px;
                                font-weight: 600;
                                vertical-alignment: center;
                            }
                        }
                        TouchArea { clicked => { root.open-drafts(); } }
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

                    Rectangle { vertical-stretch: 1; } // push the footer to the bottom

                    // ---- MANAGE FOLDERS (ORG-6) ----
                    TouchArea {
                        height: 28px;
                        clicked => { root.open-manage-folders(); }
                        HorizontalLayout {
                            padding-left: 4px;
                            Text {
                                text: "Manage folders…";
                                color: Palette.muted;
                                font-size: 13px;
                                vertical-alignment: center;
                            }
                        }
                    }

                    // ---- SETTINGS (APP-3/4) ----
                    TouchArea {
                        height: 28px;
                        clicked => { root.open-settings(); }
                        HorizontalLayout {
                            padding-left: 4px;
                            Text {
                                text: "Settings…";
                                color: Palette.muted;
                                font-size: 13px;
                                vertical-alignment: center;
                            }
                        }
                    }

                    // ---- REMOVE ACCOUNT (destructive → confirm first) ----
                    if !root.confirm-remove: TouchArea {
                        height: 30px;
                        clicked => {
                            root.confirm-remove = true;
                        }
                        HorizontalLayout {
                            padding-left: 12px;
                            Text {
                                text: "Remove account";
                                color: Palette.muted;
                                font-size: 13px;
                                vertical-alignment: center;
                            }
                        }
                    }
                    if root.confirm-remove: VerticalLayout {
                        spacing: 8px;
                        Text {
                            text: "Remove this account's local copy from this device? Your mail stays on the server.";
                            color: Palette.text;
                            font-size: 12px;
                            wrap: word-wrap;
                        }
                        HorizontalLayout {
                            spacing: 8px;
                            Rectangle {
                                height: 32px;
                                border-radius: 8px;
                                background: Palette.surface;
                                border-width: 1px;
                                border-color: Palette.danger-strong;
                                HorizontalLayout {
                                    alignment: center;
                                    Text {
                                        text: "Remove";
                                        color: Palette.danger-strong;
                                        font-size: 13px;
                                        font-weight: 600;
                                        vertical-alignment: center;
                                    }
                                }
                                TouchArea {
                                    clicked => {
                                        root.remove-account();
                                        root.confirm-remove = false;
                                    }
                                }
                            }
                            Rectangle {
                                height: 32px;
                                border-radius: 8px;
                                background: Palette.surface;
                                border-width: 1px;
                                border-color: Palette.divider;
                                HorizontalLayout {
                                    alignment: center;
                                    Text {
                                        text: "Cancel";
                                        color: Palette.text;
                                        font-size: 13px;
                                        vertical-alignment: center;
                                    }
                                }
                                TouchArea {
                                    clicked => {
                                        root.confirm-remove = false;
                                    }
                                }
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
                            if root.viewing-trash: Rectangle {
                                width: 110px;
                                height: 32px;
                                y: 10px;
                                border-radius: 8px;
                                background: Palette.surface;
                                border-width: 1px;
                                border-color: Palette.danger-strong;
                                HorizontalLayout {
                                    alignment: center;
                                    Text {
                                        text: "Empty Trash";
                                        color: Palette.danger-strong;
                                        font-size: 13px;
                                        font-weight: 600;
                                        vertical-alignment: center;
                                    }
                                }
                                TouchArea { clicked => { root.empty-trash(); } }
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
                    // search bar (SEARCH-1/2/3) — offline FTS over the local index, instant
                    Rectangle {
                        height: 48px;
                        background: Palette.surface;
                        HorizontalLayout {
                            padding-left: 14px;
                            padding-right: 14px;
                            padding-top: 7px;
                            padding-bottom: 7px;
                            spacing: 8px;
                            searchfield := Field {
                                placeholder: "Search — try from:, subject:, has:attachment";
                                text <=> root.search-query;
                                horizontal-stretch: 1;
                                edited => { root.search-edited(); }
                            }
                            // cross-account toggle (SEARCH-5) — only meaningful with >1 account
                            if root.accounts.length > 1: Text {
                                text: root.search-all ? "All accounts ✓" : "All accounts";
                                color: root.search-all ? Palette.accent-strong : Palette.muted;
                                font-size: 13px;
                                font-weight: 600;
                                vertical-alignment: center;
                                TouchArea { clicked => { root.toggle-search-all(); } }
                            }
                            if root.searching: Text {
                                text: root.search-count + " found · Clear";
                                color: Palette.accent-strong;
                                font-size: 13px;
                                font-weight: 600;
                                vertical-alignment: center;
                                TouchArea { clicked => { root.clear-search(); } }
                            }
                        }
                        Rectangle { y: parent.height - 1px; height: 1px; background: Palette.divider; }
                    }
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
                    // calm sync-progress line (distinct from the danger error banner above)
                    if root.sync-status != "": Rectangle {
                        height: 30px;
                        background: Palette.surface;
                        HorizontalLayout {
                            padding-left: 14px;
                            padding-right: 14px;
                            spacing: 8px;
                            Rectangle {
                                width: 8px;
                                height: 8px;
                                y: 11px;
                                border-radius: 4px;
                                background: Palette.accent;
                            }
                            Text {
                                text: root.sync-status;
                                color: Palette.muted;
                                font-size: 12px;
                                vertical-alignment: center;
                                overflow: elide;
                            }
                        }
                        Rectangle { y: parent.height - 1px; height: 1px; background: Palette.divider; }
                    }
                    // bulk-action bar (ORG-7) — shown while messages are multi-selected
                    if root.selected-count > 0: Rectangle {
                        height: 36px;
                        background: Palette.accent-quiet;
                        HorizontalLayout {
                            padding-left: 14px;
                            padding-right: 14px;
                            spacing: 16px;
                            Text {
                                text: root.selected-count + " selected";
                                color: Palette.text;
                                font-size: 13px;
                                vertical-alignment: center;
                                horizontal-stretch: 1;
                            }
                            Text {
                                text: "Archive";
                                color: Palette.accent-strong;
                                font-size: 13px;
                                font-weight: 600;
                                vertical-alignment: center;
                                TouchArea { clicked => { root.bulk-archive(); } }
                            }
                            Text {
                                text: "Delete";
                                color: Palette.danger-strong;
                                font-size: 13px;
                                font-weight: 600;
                                vertical-alignment: center;
                                TouchArea { clicked => { root.bulk-trash(); } }
                            }
                            Text {
                                text: "Star";
                                color: Palette.accent-strong;
                                font-size: 13px;
                                font-weight: 600;
                                vertical-alignment: center;
                                TouchArea { clicked => { root.bulk-star(); } }
                            }
                            Text {
                                text: "Mark read";
                                color: Palette.accent-strong;
                                font-size: 13px;
                                font-weight: 600;
                                vertical-alignment: center;
                                TouchArea { clicked => { root.bulk-mark-read(); } }
                            }
                            Text {
                                text: "Clear";
                                color: Palette.muted;
                                font-size: 13px;
                                vertical-alignment: center;
                                TouchArea { clicked => { root.clear-selection(); } }
                            }
                        }
                    }
                    // calm empty state (APP-2 polish): nothing in this folder / no search results
                    if root.messages.length == 0: Rectangle {
                        vertical-stretch: 1;
                        background: Palette.surface;
                        Text {
                            text: root.searching
                                ? "No messages match your search."
                                : (root.refreshing ? "Loading…" : "No messages here yet.");
                            color: Palette.muted;
                            font-size: 14px;
                            horizontal-alignment: center;
                            vertical-alignment: center;
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
                            // open-message hit area — declared BELOW the content so the per-row
                            // checkbox (declared later, inside the layout) wins in its corner.
                            TouchArea { clicked => { root.message-selected(m); } }
                            HorizontalLayout {
                                padding: 12px;
                                spacing: 10px;
                                // multi-select checkbox (ORG-7)
                                Rectangle {
                                    width: 18px;
                                    Rectangle {
                                        y: (parent.height - 16px) / 2;
                                        width: 16px;
                                        height: 16px;
                                        border-radius: 4px;
                                        border-width: 1px;
                                        border-color: m.selected ? Palette.accent : Palette.divider;
                                        background: m.selected ? Palette.accent : Palette.surface;
                                        Text {
                                            text: m.selected ? "✓" : "";
                                            color: white;
                                            font-size: 11px;
                                            horizontal-alignment: center;
                                            vertical-alignment: center;
                                        }
                                    }
                                    TouchArea { clicked => { root.toggle-select(m.id); } }
                                }
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
                                        text: (m.starred ? "★ " : "") + m.subject;
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
                                    // conversation size (READ-5) — shown only when threaded
                                    if m.thread-count > 1: Text {
                                        text: "conversation · " + m.thread-count;
                                        color: Palette.accent-strong;
                                        font-size: 11px;
                                        horizontal-alignment: right;
                                    }
                                }
                            }
                            Rectangle {
                                y: parent.height - 1px;
                                height: 1px;
                                background: Palette.divider;
                            }
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
                    HorizontalLayout {
                        height: 22px;
                        alignment: start;
                        spacing: 16px;
                        Text {
                            text: "Reply";
                            color: Palette.accent-strong;
                            font-size: 13px;
                            font-weight: 600;
                            TouchArea { clicked => { root.reply(); } }
                        }
                        Text {
                            text: "Reply all";
                            color: Palette.accent-strong;
                            font-size: 13px;
                            font-weight: 600;
                            TouchArea { clicked => { root.reply-all(); } }
                        }
                        Text {
                            text: "Forward";
                            color: Palette.accent-strong;
                            font-size: 13px;
                            font-weight: 600;
                            TouchArea { clicked => { root.forward(); } }
                        }
                        Text {
                            text: root.r-starred ? "★ Starred" : "☆ Star";
                            color: root.r-starred ? Palette.accent-strong : Palette.muted;
                            font-size: 13px;
                            font-weight: 600;
                            TouchArea { clicked => { root.toggle-star(root.selected-message); } }
                        }
                        Text {
                            text: "Archive";
                            color: Palette.accent-strong;
                            font-size: 13px;
                            font-weight: 600;
                            TouchArea { clicked => { root.archive-message(root.selected-message); } }
                        }
                        Text {
                            text: "Delete";
                            color: Palette.danger-strong;
                            font-size: 13px;
                            font-weight: 600;
                            TouchArea { clicked => { root.trash-message(root.selected-message); } }
                        }
                        Text {
                            text: "Move…";
                            color: Palette.accent-strong;
                            font-size: 13px;
                            font-weight: 600;
                            TouchArea { clicked => { root.open-move(root.selected-message); } }
                        }
                        Text {
                            text: root.viewing-junk ? "Not spam" : "Spam";
                            color: Palette.accent-strong;
                            font-size: 13px;
                            font-weight: 600;
                            TouchArea { clicked => { root.toggle-junk(root.selected-message); } }
                        }
                        Text {
                            text: "Mark as unread";
                            color: Palette.muted;
                            font-size: 13px;
                            TouchArea { clicked => { root.mark-unread(root.selected-message); } }
                        }
                    }
                    // remote content blocked (PRIV-3) + per-message opt-in (PRIV-2)
                    if root.remote-blocked: Rectangle {
                        height: 32px;
                        border-radius: 8px;
                        background: Palette.accent-quiet;
                        HorizontalLayout {
                            padding-left: 10px;
                            padding-right: 10px;
                            spacing: 8px;
                            Text {
                                text: "Remote content blocked";
                                color: Palette.text;
                                font-size: 12px;
                                vertical-alignment: center;
                                horizontal-stretch: 1;
                            }
                            TouchArea {
                                width: 140px;
                                clicked => { root.load-remote(); }
                                Text {
                                    text: "Load remote images";
                                    color: Palette.text;
                                    font-size: 12px;
                                    font-weight: 600;
                                    vertical-alignment: center;
                                    horizontal-alignment: right;
                                }
                            }
                        }
                    }
                    ScrollView {
                        Text {
                            text: root.r-body;
                            color: Palette.text;
                            wrap: word-wrap;
                        }
                    }
                    // attachments (view only — saving comes later)
                    if root.r-attachments.length > 0: VerticalLayout {
                        spacing: 4px;
                        Rectangle { height: 1px; background: Palette.divider; }
                        Text {
                            text: "Attachments";
                            color: Palette.muted;
                            font-size: 12px;
                            font-weight: 600;
                        }
                        for a in root.r-attachments: HorizontalLayout {
                            spacing: 8px;
                            Text { text: "[paperclip]"; color: Palette.muted; font-size: 13px; }
                            Text { text: a; color: Palette.text; font-size: 13px; overflow: elide; }
                        }
                    }
                }
            }
        }

        // ---- ADD-ACCOUNT FORM (no account yet, or reconnect) ----
        if root.needs-setup || root.adding-account: Rectangle {
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
                            HorizontalLayout {
                                Text {
                                    text: root.adding-account ? "Add another account" : "Add your account";
                                    color: Palette.text;
                                    font-size: 20px;
                                    font-weight: 600;
                                    horizontal-stretch: 1;
                                }
                                if root.adding-account: Text {
                                    text: "Cancel";
                                    color: Palette.accent-strong;
                                    font-size: 14px;
                                    font-weight: 600;
                                    vertical-alignment: center;
                                    TouchArea { clicked => { root.cancel-add-account(); } }
                                }
                            }
                            Text {
                                text: "Connect over IMAP. Your details stay on this device.";
                                color: Palette.muted;
                                font-size: 13px;
                                wrap: word-wrap;
                            }
                            Text { text: "Email"; color: Palette.muted; font-size: 12px; }
                            Field { placeholder: "you@example.com"; text <=> root.f-email; }
                            Text { text: "Display name (optional)"; color: Palette.muted; font-size: 12px; }
                            Field { placeholder: "Your name"; text <=> root.f-name; }
                            Text { text: "IMAP server"; color: Palette.muted; font-size: 12px; }
                            Field { placeholder: "imap.example.com"; text <=> root.f-host; }
                            Text { text: "Port"; color: Palette.muted; font-size: 12px; }
                            Field { placeholder: "993"; text <=> root.f-port; }
                            Text { text: "Username"; color: Palette.muted; font-size: 12px; }
                            Field { placeholder: "usually your email"; text <=> root.f-user; }
                            Text { text: "Password"; color: Palette.muted; font-size: 12px; }
                            Field { password: true; text <=> root.f-pass; }
                            Text { text: "SMTP server (for sending)"; color: Palette.muted; font-size: 12px; }
                            Field { placeholder: "smtp.example.com"; text <=> root.f-smtp-host; }
                            Text { text: "SMTP port"; color: Palette.muted; font-size: 12px; }
                            Field { placeholder: root.f-smtp-starttls ? "587" : "465"; text <=> root.f-smtp-port; }
                            HorizontalLayout {
                                spacing: 8px;
                                Rectangle {
                                    width: 18px;
                                    height: 18px;
                                    border-radius: 4px;
                                    border-width: 1px;
                                    border-color: Palette.divider;
                                    background: root.f-smtp-starttls ? Palette.accent : Palette.surface;
                                    Text {
                                        text: root.f-smtp-starttls ? "✓" : "";
                                        color: white;
                                        font-size: 12px;
                                        horizontal-alignment: center;
                                        vertical-alignment: center;
                                    }
                                    TouchArea { clicked => { root.f-smtp-starttls = !root.f-smtp-starttls; } }
                                }
                                Text {
                                    text: "Use STARTTLS (port 587). Off = implicit TLS (465).";
                                    color: Palette.muted;
                                    font-size: 12px;
                                    vertical-alignment: center;
                                }
                            }
                            Text { text: "Signature (optional)"; color: Palette.muted; font-size: 12px; }
                            Field {
                                height: 72px;
                                multiline: true;
                                placeholder: "e.g. — Your Name";
                                text <=> root.f-signature;
                            }
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

        // ---- COMPOSE (M4 send) — overlay on top of everything ----
        if root.composing: Rectangle {
            background: #0d171b99; // dim the backdrop
            TouchArea {} // swallow clicks behind the card
            Rectangle {
                width: min(640px, parent.width - 80px);
                height: min(560px, parent.height - 80px);
                x: (parent.width - self.width) / 2;
                y: (parent.height - self.height) / 2;
                background: Palette.surface;
                border-radius: 12px;
                VerticalLayout {
                    padding: 20px;
                    spacing: 10px;
                    Text {
                        text: "New message";
                        color: Palette.text;
                        font-size: 18px;
                        font-weight: 700;
                    }
                    Text { text: "To"; color: Palette.muted; font-size: 12px; }
                    Field {
                        placeholder: "name@example.com, …";
                        text <=> root.c-to;
                        edited => { root.to-edited(); }
                    }
                    if root.c-suggestions.length > 0: Rectangle {
                        background: Palette.surface;
                        border-width: 1px;
                        border-color: Palette.divider;
                        border-radius: 8px;
                        VerticalLayout {
                            for sug in root.c-suggestions: Rectangle {
                                height: 28px;
                                background: st.has-hover ? Palette.accent-quiet : transparent;
                                st := TouchArea { clicked => { root.pick-suggestion(sug); } }
                                Text {
                                    text: sug;
                                    x: 9px;
                                    color: Palette.text;
                                    font-size: 13px;
                                    vertical-alignment: center;
                                }
                            }
                        }
                    }
                    Text { text: "Cc (optional)"; color: Palette.muted; font-size: 12px; }
                    Field {
                        placeholder: "name@example.com, …";
                        text <=> root.c-cc;
                        edited => { root.cc-edited(); }
                    }
                    if root.c-cc-suggestions.length > 0: Rectangle {
                        background: Palette.surface;
                        border-width: 1px;
                        border-color: Palette.divider;
                        border-radius: 8px;
                        VerticalLayout {
                            for sug in root.c-cc-suggestions: Rectangle {
                                height: 28px;
                                background: ct.has-hover ? Palette.accent-quiet : transparent;
                                ct := TouchArea { clicked => { root.pick-cc-suggestion(sug); } }
                                Text {
                                    text: sug;
                                    x: 9px;
                                    color: Palette.text;
                                    font-size: 13px;
                                    vertical-alignment: center;
                                }
                            }
                        }
                    }
                    Text { text: "Subject"; color: Palette.muted; font-size: 12px; }
                    Field { text <=> root.c-subject; }
                    Field {
                        multiline: true;
                        placeholder: "Write your message…";
                        text <=> root.c-body;
                        vertical-stretch: 1;
                    }
                    // attachments (SEND-4): type/paste a path + Attach (native picker is a follow-up)
                    HorizontalLayout {
                        spacing: 8px;
                        Field {
                            placeholder: "/path/to/a/file to attach";
                            text <=> root.c-attach-path;
                            horizontal-stretch: 1;
                        }
                        Rectangle {
                            width: 90px;
                            height: 38px;
                            border-radius: 8px;
                            background: Palette.bg;
                            border-width: 1px;
                            border-color: Palette.divider;
                            Text {
                                text: "Browse…";
                                color: Palette.text;
                                vertical-alignment: center;
                                horizontal-alignment: center;
                            }
                            TouchArea { clicked => { root.browse-file(); } }
                        }
                        Rectangle {
                            width: 90px;
                            height: 38px;
                            border-radius: 8px;
                            background: Palette.bg;
                            border-width: 1px;
                            border-color: Palette.divider;
                            Text {
                                text: "Attach";
                                color: Palette.text;
                                vertical-alignment: center;
                                horizontal-alignment: center;
                            }
                            TouchArea { clicked => { root.attach-file(); } }
                        }
                    }
                    for a[i] in root.c-attachments: HorizontalLayout {
                        spacing: 8px;
                        Text {
                            text: a;
                            color: Palette.muted;
                            font-size: 12px;
                            vertical-alignment: center;
                            horizontal-stretch: 1;
                            overflow: elide;
                        }
                        Text {
                            text: "Remove";
                            color: Palette.accent-strong;
                            font-size: 12px;
                            TouchArea { clicked => { root.remove-attachment(i); } }
                        }
                    }
                    // Markdown formatting toggle (SEND-6)
                    HorizontalLayout {
                        spacing: 8px;
                        Rectangle {
                            width: 18px;
                            height: 18px;
                            border-radius: 4px;
                            border-width: 1px;
                            border-color: Palette.divider;
                            background: root.c-markdown ? Palette.accent : Palette.surface;
                            Text {
                                text: root.c-markdown ? "✓" : "";
                                color: white;
                                font-size: 12px;
                                horizontal-alignment: center;
                                vertical-alignment: center;
                            }
                            TouchArea { clicked => { root.c-markdown = !root.c-markdown; } }
                        }
                        Text {
                            text: "Format with Markdown (bold, lists, links)";
                            color: Palette.muted;
                            font-size: 12px;
                            vertical-alignment: center;
                        }
                    }
                    if root.compose-status != "": Text {
                        text: root.compose-status;
                        color: Palette.danger-strong;
                        font-size: 13px;
                        wrap: word-wrap;
                    }
                    HorizontalLayout {
                        spacing: 10px;
                        alignment: end;
                        Rectangle {
                            width: 100px;
                            height: 40px;
                            border-radius: 8px;
                            background: Palette.bg;
                            border-width: 1px;
                            border-color: Palette.divider;
                            Text {
                                text: "Cancel";
                                color: Palette.text;
                                vertical-alignment: center;
                                horizontal-alignment: center;
                            }
                            TouchArea {
                                enabled: !root.sending;
                                clicked => { root.cancel-compose(); }
                            }
                        }
                        Rectangle {
                            width: 110px;
                            height: 40px;
                            border-radius: 8px;
                            background: Palette.bg;
                            border-width: 1px;
                            border-color: Palette.divider;
                            Text {
                                text: "Save draft";
                                color: Palette.text;
                                vertical-alignment: center;
                                horizontal-alignment: center;
                            }
                            TouchArea {
                                enabled: !root.sending;
                                clicked => { root.save-draft(); }
                            }
                        }
                        Rectangle {
                            width: 120px;
                            height: 40px;
                            border-radius: 8px;
                            background: Palette.accent-strong;
                            opacity: root.sending ? 0.6 : 1.0;
                            Text {
                                text: root.sending ? "Sending…" : "Send";
                                color: white;
                                font-weight: 600;
                                vertical-alignment: center;
                                horizontal-alignment: center;
                            }
                            TouchArea {
                                enabled: !root.sending;
                                clicked => { root.send-message(); }
                            }
                        }
                    }
                }
            }
        }

        // ---- DRAFTS (SEND-5) — pick a saved draft to resume ----
        if root.viewing-drafts: Rectangle {
            background: #0d171b99;
            TouchArea {}
            Rectangle {
                width: min(560px, parent.width - 80px);
                height: min(560px, parent.height - 80px);
                x: (parent.width - self.width) / 2;
                y: (parent.height - self.height) / 2;
                background: Palette.surface;
                border-radius: 12px;
                VerticalLayout {
                    padding: 20px;
                    spacing: 10px;
                    HorizontalLayout {
                        Text {
                            text: "Drafts";
                            color: Palette.text;
                            font-size: 18px;
                            font-weight: 700;
                            horizontal-stretch: 1;
                        }
                        Text {
                            text: "Close";
                            color: Palette.accent-strong;
                            font-size: 14px;
                            font-weight: 600;
                            TouchArea { clicked => { root.close-drafts(); } }
                        }
                    }
                    if root.drafts.length == 0: Text {
                        text: "No saved drafts.";
                        color: Palette.muted;
                    }
                    ListView {
                        vertical-stretch: 1;
                        for d in root.drafts: Rectangle {
                            height: 56px;
                            border-radius: 8px;
                            background: touch.has-hover ? Palette.accent-quiet : transparent;
                            touch := TouchArea { clicked => { root.resume-draft(d.id); } }
                            VerticalLayout {
                                padding: 8px;
                                Text {
                                    text: d.subject == "" ? "(no subject)" : d.subject;
                                    color: Palette.text;
                                    font-weight: 600;
                                    overflow: elide;
                                }
                                Text {
                                    text: d.recipients == "" ? "(no recipients)" : d.recipients;
                                    color: Palette.muted;
                                    font-size: 12px;
                                    overflow: elide;
                                }
                            }
                        }
                    }
                }
            }
        }

        // ---- MOVE TO… (ORG-3) — pick a destination folder ----
        if root.picking-folder: Rectangle {
            background: #0d171b99;
            TouchArea {}
            Rectangle {
                width: min(420px, parent.width - 80px);
                height: min(480px, parent.height - 80px);
                x: (parent.width - self.width) / 2;
                y: (parent.height - self.height) / 2;
                background: Palette.surface;
                border-radius: 12px;
                VerticalLayout {
                    padding: 20px;
                    spacing: 10px;
                    HorizontalLayout {
                        Text {
                            text: "Move to…";
                            color: Palette.text;
                            font-size: 18px;
                            font-weight: 700;
                            horizontal-stretch: 1;
                        }
                        Text {
                            text: "Cancel";
                            color: Palette.accent-strong;
                            font-size: 14px;
                            font-weight: 600;
                            TouchArea { clicked => { root.close-move(); } }
                        }
                    }
                    ListView {
                        vertical-stretch: 1;
                        for f in root.move-folders: Rectangle {
                            height: 36px;
                            border-radius: 8px;
                            background: mt.has-hover ? Palette.accent-quiet : transparent;
                            mt := TouchArea { clicked => { root.move-to(f); } }
                            Text {
                                text: f;
                                x: 9px;
                                color: Palette.text;
                                vertical-alignment: center;
                            }
                        }
                    }
                }
            }
        }

        // ---- MANAGE FOLDERS (ORG-6) — create / rename / delete ----
        if root.managing-folders: Rectangle {
            background: #0d171b99;
            TouchArea {}
            Rectangle {
                width: min(480px, parent.width - 80px);
                height: min(520px, parent.height - 80px);
                x: (parent.width - self.width) / 2;
                y: (parent.height - self.height) / 2;
                background: Palette.surface;
                border-radius: 12px;
                VerticalLayout {
                    padding: 20px;
                    spacing: 10px;
                    HorizontalLayout {
                        Text {
                            text: "Manage folders";
                            color: Palette.text;
                            font-size: 18px;
                            font-weight: 700;
                            horizontal-stretch: 1;
                        }
                        Text {
                            text: "Close";
                            color: Palette.accent-strong;
                            font-size: 14px;
                            font-weight: 600;
                            TouchArea { clicked => { root.close-manage-folders(); } }
                        }
                    }
                    Text {
                        text: "Folder name (for Create, or Rename target):";
                        color: Palette.muted;
                        font-size: 12px;
                    }
                    HorizontalLayout {
                        spacing: 8px;
                        Field { placeholder: "e.g. Projects"; text <=> root.mf-name; horizontal-stretch: 1; }
                        Rectangle {
                            width: 90px;
                            height: 38px;
                            border-radius: 8px;
                            background: Palette.accent-strong;
                            Text {
                                text: "Create";
                                color: white;
                                font-weight: 600;
                                vertical-alignment: center;
                                horizontal-alignment: center;
                            }
                            TouchArea { clicked => { root.create-folder(); } }
                        }
                    }
                    ListView {
                        vertical-stretch: 1;
                        for f in root.manage-folders: Rectangle {
                            height: 38px;
                            HorizontalLayout {
                                padding-left: 4px;
                                spacing: 10px;
                                Text {
                                    text: f;
                                    color: Palette.text;
                                    vertical-alignment: center;
                                    horizontal-stretch: 1;
                                    overflow: elide;
                                }
                                Text {
                                    text: "Rename→";
                                    color: Palette.accent-strong;
                                    font-size: 13px;
                                    vertical-alignment: center;
                                    TouchArea { clicked => { root.rename-folder(f); } }
                                }
                                Text {
                                    text: "Delete";
                                    color: Palette.danger-strong;
                                    font-size: 13px;
                                    vertical-alignment: center;
                                    TouchArea { clicked => { root.delete-folder(f); } }
                                }
                            }
                        }
                    }
                }
            }
        }

        // ---- SETTINGS (APP-3/4) ----
        if root.showing-settings: Rectangle {
            background: #0d171b99;
            TouchArea {}
            Rectangle {
                width: min(420px, parent.width - 80px);
                height: min(260px, parent.height - 80px);
                x: (parent.width - self.width) / 2;
                y: (parent.height - self.height) / 2;
                background: Palette.surface;
                border-radius: 12px;
                VerticalLayout {
                    padding: 20px;
                    spacing: 14px;
                    HorizontalLayout {
                        Text {
                            text: "Settings";
                            color: Palette.text;
                            font-size: 18px;
                            font-weight: 700;
                            horizontal-stretch: 1;
                        }
                        Text {
                            text: "Close";
                            color: Palette.accent-strong;
                            font-size: 14px;
                            font-weight: 600;
                            TouchArea { clicked => { root.close-settings(); } }
                        }
                    }
                    HorizontalLayout {
                        spacing: 12px;
                        Text {
                            text: "Theme";
                            color: Palette.text;
                            font-size: 14px;
                            vertical-alignment: center;
                            horizontal-stretch: 1;
                        }
                        Rectangle {
                            width: 130px;
                            height: 36px;
                            border-radius: 8px;
                            background: Palette.accent-strong;
                            Text {
                                text: Palette.dark ? "🌙 Dark" : "☀ Light";
                                color: white;
                                font-weight: 600;
                                vertical-alignment: center;
                                horizontal-alignment: center;
                            }
                            TouchArea { clicked => { root.toggle-theme(); } }
                        }
                    }
                    Text {
                        text: "No telemetry, no tracking — settings stay on this device.";
                        color: Palette.muted;
                        font-size: 12px;
                        wrap: word-wrap;
                    }
                    Rectangle { vertical-stretch: 1; }
                }
            }
        }
    }
}

/// Load the first account's drafts as display rows (newest first).
fn load_draft_items(store: &Store) -> Vec<DraftItem> {
    let Some(acc) = store
        .list_accounts()
        .ok()
        .and_then(|a| a.into_iter().next())
    else {
        return Vec::new();
    };
    store
        .list_drafts(acc.id)
        .unwrap_or_default()
        .into_iter()
        .map(|d| {
            let recipients = [d.content.to.as_str(), d.content.cc.as_str()]
                .iter()
                .filter(|s| !s.is_empty())
                .copied()
                .collect::<Vec<_>>()
                .join(", ");
            DraftItem {
                id: d.id as i32,
                subject: d.content.subject.into(),
                recipients: recipients.into(),
                date: viewmodel::format_date(Some(d.updated_at)).into(),
            }
        })
        .collect()
}

/// Refresh the compose attachment display model ("name · size" per file) from the real list.
fn refresh_attachments(
    model: &VecModel<SharedString>,
    atts: &[geleit_engine::message::Attachment],
) {
    let rows: Vec<SharedString> = atts
        .iter()
        .map(|a| viewmodel::attachment_label(Some(&a.filename), a.data.len() as u64).into())
        .collect();
    model.set_vec(rows);
}

/// Open the desktop's native file chooser (zenity, then kdialog) and return the chosen path. Runs
/// the chooser as a **separate process** (no in-process GTK loop to clash with Slint/webkit). `None`
/// if the user cancelled or no chooser is installed (the manual path field is the fallback).
fn pick_file_via_dialog() -> Option<String> {
    for (cmd, args) in [
        ("zenity", &["--file-selection"][..]),
        ("kdialog", &["--getopenfilename"][..]),
    ] {
        match std::process::Command::new(cmd).args(args).output() {
            Ok(out) if out.status.success() => {
                let path = String::from_utf8_lossy(&out.stdout).trim().to_owned();
                return (!path.is_empty()).then_some(path);
            }
            Ok(_) => return None, // chooser ran but the user cancelled
            Err(_) => continue,   // not installed → try the next
        }
    }
    None
}

/// The first account's signature (empty if none) — appended to composed messages.
fn account_signature(store: &Store) -> String {
    store
        .list_accounts()
        .ok()
        .and_then(|a| a.into_iter().next())
        .and_then(|acc| store.signature(acc.id).ok().flatten())
        .unwrap_or_default()
}

/// Map full-text search hits (SEARCH-1) to list rows — relevance order, no threading (results span
/// folders) and nothing pre-selected.
/// Build search-result rows. When `all_accounts`, search every account and tag each row with its
/// owning account (so opening it can switch context, SEARCH-5); otherwise search `account_id` only
/// and leave `account = 0` (no switch needed).
fn search_result_items(
    store: &Store,
    account_id: i64,
    query: &str,
    all_accounts: bool,
) -> Vec<MessageItem> {
    let rows: Vec<(geleit_store::MessageHeader, i64)> = if all_accounts {
        store.search_all_accounts(query, 500).unwrap_or_default()
    } else {
        store
            .search_messages(account_id, query, 500)
            .unwrap_or_default()
            .into_iter()
            .map(|h| (h, 0)) // 0 = current account, no switch on open
            .collect()
    };
    rows.iter()
        .map(|(h, acc)| {
            let vm = viewmodel::message_vm(h);
            MessageItem {
                id: h.id as i32,
                sender: vm.sender.into(),
                subject: vm.subject.into(),
                snippet: vm.snippet.into(),
                date: vm.date.into(),
                unread: vm.unread,
                attachment: vm.attachment,
                thread_count: 1,
                starred: vm.starred,
                selected: false,
                account: *acc as i32,
            }
        })
        .collect()
}

fn load_messages(store: &Store, folder_id: i64) -> Vec<MessageItem> {
    let headers = store
        .messages_in_folder(folder_id, 1000)
        .unwrap_or_default();

    // Conversation size per message (READ-5): group by Message-ID / In-Reply-To.
    let items: Vec<geleit_engine::thread::ThreadItem> = headers
        .iter()
        .map(|h| geleit_engine::thread::ThreadItem {
            message_id: h.message_id.as_deref(),
            in_reply_to: h.in_reply_to.as_deref(),
        })
        .collect();
    let mut thread_size = vec![1usize; headers.len()];
    for grp in geleit_engine::thread::group(&items) {
        for &i in &grp {
            thread_size[i] = grp.len();
        }
    }

    headers
        .iter()
        .enumerate()
        .map(|(i, h)| {
            let vm = viewmodel::message_vm(h);
            MessageItem {
                id: h.id as i32,
                sender: vm.sender.into(),
                subject: vm.subject.into(),
                snippet: vm.snippet.into(),
                date: vm.date.into(),
                unread: vm.unread,
                attachment: vm.attachment,
                thread_count: thread_size[i] as i32,
                starred: vm.starred,
                selected: false,
                account: 0, // folder view is always the current account
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

/// Flip one row's `starred` flag in place (keeps scroll, avoids a full re-query).
fn flip_starred(model: &VecModel<MessageItem>, id: i32, starred: bool) {
    for i in 0..model.row_count() {
        if let Some(mut row) = model.row_data(i) {
            if row.id == id {
                row.starred = starred;
                model.set_row_data(i, row);
                break;
            }
        }
    }
}

/// Flip one row's `selected` flag in place (multi-select, ORG-7).
fn flip_selected_row(model: &VecModel<MessageItem>, id: i32, selected: bool) {
    for i in 0..model.row_count() {
        if let Some(mut row) = model.row_data(i) {
            if row.id == id {
                row.selected = selected;
                model.set_row_data(i, row);
                break;
            }
        }
    }
}

/// Bulk-move all `selected` messages to `target` (archive/trash): optimistic local-remove of each,
/// clear the selection, then one worker writes the moves back. Failures return on refresh.
#[allow(clippy::too_many_arguments)]
fn bulk_move(
    ui: &Main,
    store: &Store,
    messages: &VecModel<MessageItem>,
    folders: &VecModel<SharedString>,
    view: &HtmlView,
    db_path: &str,
    secrets: &Arc<OsSecretStore>,
    account_id: i64,
    selected: &Rc<RefCell<HashSet<i32>>>,
    target: &str,
) {
    let source = folders
        .row_data(ui.get_selected_folder() as usize)
        .map(|s| s.to_string())
        .unwrap_or_else(|| "INBOX".to_owned());
    let ids: Vec<i32> = selected.borrow().iter().copied().collect();
    let mut uids = Vec::new();
    for id in ids {
        if let Some(uid) = store
            .header_by_id(id.into())
            .ok()
            .flatten()
            .and_then(|h| h.uid)
        {
            uids.push(uid);
        }
        let _ = store.delete_message(id.into());
        remove_row(messages, id);
    }
    selected.borrow_mut().clear();
    ui.set_selected_count(0);
    ui.set_selected_message(0);
    hide_html(view);
    if uids.is_empty() {
        return;
    }
    let weak = ui.as_weak();
    let (db_path, secrets, target) = (db_path.to_owned(), secrets.clone(), target.to_owned());
    std::thread::spawn(move || {
        let mut any_err = false;
        for uid in uids {
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                refresh::run_move(
                    &db_path, &*secrets, account_id, &source, uid as u32, &target,
                )
            }))
            .unwrap_or(Err(String::new()));
            any_err |= r.is_err();
        }
        if any_err {
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = weak.upgrade() {
                    ui.set_status(
                        "Some messages couldn't be moved — they'll return on the next refresh."
                            .into(),
                    );
                }
            });
        }
    });
}

/// Bulk-star all `selected` messages (ORG-7): optimistic local flag + clear selection + write-back.
#[allow(clippy::too_many_arguments)]
fn bulk_star(
    ui: &Main,
    store: &Store,
    messages: &VecModel<MessageItem>,
    folders: &VecModel<SharedString>,
    db_path: &str,
    secrets: &Arc<OsSecretStore>,
    account_id: i64,
    selected: &Rc<RefCell<HashSet<i32>>>,
) {
    let source = folders
        .row_data(ui.get_selected_folder() as usize)
        .map(|s| s.to_string())
        .unwrap_or_else(|| "INBOX".to_owned());
    let ids: Vec<i32> = selected.borrow().iter().copied().collect();
    let mut uids = Vec::new();
    for id in ids {
        if let Ok(Some(uid)) = store.set_flagged(id.into(), true) {
            uids.push(uid);
        }
        flip_starred(messages, id, true);
        flip_selected_row(messages, id, false);
    }
    selected.borrow_mut().clear();
    ui.set_selected_count(0);
    if uids.is_empty() {
        return;
    }
    let (db_path, secrets) = (db_path.to_owned(), secrets.clone());
    std::thread::spawn(move || {
        for uid in uids {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                refresh::run_set_flag(&db_path, &*secrets, account_id, &source, uid as u32, true)
            }));
        }
    });
}

/// Fire-and-forget read-state (`\Seen`) write-back on a worker (SYNC-5). Best-effort, like the star
/// write-back: read state stays correct locally even if the server push fails.
fn spawn_set_seen(
    db_path: &str,
    secrets: &Arc<OsSecretStore>,
    account_id: i64,
    folder: String,
    uid: u32,
    seen: bool,
) {
    let (db_path, secrets) = (db_path.to_owned(), secrets.clone());
    std::thread::spawn(move || {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            refresh::run_set_seen(&db_path, &*secrets, account_id, &folder, uid, seen)
        }));
    });
}

/// Remove one row by id (optimistic archive/trash/move — keeps scroll).
fn remove_row(model: &VecModel<MessageItem>, id: i32) {
    for i in 0..model.row_count() {
        if model.row_data(i).is_some_and(|r| r.id == id) {
            model.remove(i);
            break;
        }
    }
}

/// Run a folder create/rename/delete on a worker; on success reload the rail + close the manager,
/// on failure surface a calm status. (ORG-6.)
fn spawn_folder_op(
    weak: slint::Weak<Main>,
    op: impl FnOnce() -> Result<(), String> + Send + 'static,
) {
    std::thread::spawn(move || {
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(op))
            .unwrap_or_else(|_| Err("Something went wrong.".to_owned()));
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = weak.upgrade() {
                match res {
                    Ok(()) => {
                        ui.invoke_reload();
                        ui.set_managing_folders(false);
                    }
                    Err(msg) => ui.set_status(msg.into()),
                }
            }
        });
    });
}

/// Address autocomplete rows for `token` in `account_id` (SEND-9): suggestions from mail history,
/// minus an exact match of what's already typed. Empty token → no rows.
fn address_suggestions(store: &Store, account_id: i64, token: &str) -> Vec<SharedString> {
    if token.is_empty() {
        return Vec::new();
    }
    store
        .suggest_addresses(account_id, token, 6)
        .unwrap_or_default()
        .into_iter()
        .filter(|s| !s.eq_ignore_ascii_case(token))
        .map(Into::into)
        .collect()
}

/// The folder names currently in the rail, as owned strings.
fn folder_names(folders: &VecModel<SharedString>) -> Vec<String> {
    (0..folders.row_count())
        .filter_map(|i| folders.row_data(i))
        .map(|s| s.to_string())
        .collect()
}

/// Archive/trash/move a message: optimistic local-remove + reading-pane clear, then a worker writes
/// the IMAP move back. On failure the row returns on the next refresh (no loss).
#[allow(clippy::too_many_arguments)]
fn perform_move(
    ui: &Main,
    store: &Store,
    messages: &VecModel<MessageItem>,
    folders: &VecModel<SharedString>,
    view: &HtmlView,
    db_path: &str,
    secrets: &Arc<OsSecretStore>,
    account_id: i64,
    id: i32,
    target: &str,
) {
    let source = folders
        .row_data(ui.get_selected_folder() as usize)
        .map(|s| s.to_string())
        .unwrap_or_else(|| "INBOX".to_owned());
    if target.is_empty() || target.eq_ignore_ascii_case(&source) {
        return;
    }
    let uid = store
        .header_by_id(id.into())
        .ok()
        .flatten()
        .and_then(|h| h.uid);
    let _ = store.delete_message(id.into()); // optimistic
    remove_row(messages, id);
    ui.set_selected_message(0);
    hide_html(view);
    let Some(uid) = uid else { return }; // local-only message → nothing to move on the server
    let weak = ui.as_weak();
    let db_path = db_path.to_owned();
    let secrets = secrets.clone();
    let source = source.clone();
    let target = target.to_owned();
    std::thread::spawn(move || {
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            refresh::run_move(
                &db_path, &*secrets, account_id, &source, uid as u32, &target,
            )
        }))
        .unwrap_or_else(|_| Err("Couldn't move the message.".to_owned()));
        if let Err(_msg) = res {
            let w = weak.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = w.upgrade() {
                    ui.set_status(
                        "Couldn't move the message — it'll return on the next refresh.".into(),
                    );
                }
            });
        }
    });
}

/// Re-read account / folders / current-folder messages into the UI (UI thread only). Sets
/// `needs-setup` when there is no account, so the Add-account form shows.
fn reload_all(
    ui: &Main,
    store: &Store,
    folders_model: &VecModel<SharedString>,
    folder_ids: &RefCell<Vec<i64>>,
    messages: &VecModel<MessageItem>,
    accounts_model: &VecModel<AccountItem>,
) {
    ui.set_status(SharedString::new()); // clear any stale main-view banner
    ui.set_sync_status(SharedString::new());
    ui.set_remote_blocked(false);
    let accounts = store.list_accounts().unwrap_or_default();
    accounts_model.set_vec(
        accounts
            .iter()
            .map(|a| AccountItem {
                id: a.id as i32,
                email: a.email.as_str().into(),
            })
            .collect::<Vec<_>>(),
    );
    // The account in view (the `current-account` prop is the source of truth): keep it if it still
    // exists, else fall back to the first.
    let desired = ui.get_current_account() as i64;
    let target = accounts
        .iter()
        .find(|a| a.id == desired)
        .or_else(|| accounts.first());
    match target {
        Some(acc) => {
            ui.set_needs_setup(false);
            ui.set_account(acc.email.as_str().into());
            ui.set_current_account(acc.id as i32);
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
            ui.set_current_account(-1);
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
    // Render Slint in **software** (no GL context) so it can't collide with the embedded webview's
    // GL (webkit) — that clash caused GLXBadWindow crashes. Set before any Slint call; only if the
    // user hasn't chosen a backend, so an explicit `SLINT_BACKEND` still wins.
    if std::env::var_os("SLINT_BACKEND").is_none() {
        std::env::set_var("SLINT_BACKEND", "winit-software");
    }

    // The embedded HTML webview (S3.1) uses webkit2gtk, which needs GTK initialised + its loop
    // pumped. Best-effort: if GTK isn't available the app still runs (HTML falls back to text).
    let gtk_ready = gtk::init().is_ok();

    let db = std::env::var("GELEIT_DB").unwrap_or_else(|_| "geleit.db".to_owned());
    // Secret store backed by the OS keychain (S2.1). Send+Sync → shared across the UI + workers.
    let secrets = Arc::new(OsSecretStore::new());
    // Encrypted local store (SEC-1, ADR-0008); the key is fetched/created in the keychain.
    let store = Rc::new(refresh::open_store(&db, &*secrets)?);

    let ui = Main::new()?;
    let folders_model = Rc::new(VecModel::<SharedString>::default());
    let folder_ids = Rc::new(RefCell::new(Vec::<i64>::new()));
    let messages = Rc::new(VecModel::<MessageItem>::default());
    let html_view = Rc::new(HtmlView::default());
    // the open HTML message's remote-allowed document, rendered if the user opts in (PRIV-2)
    let current_allowed = Rc::new(RefCell::new(Option::<String>::None));
    // threading headers (in_reply_to, references) for the message being composed, if it's a reply
    type ComposeThread = Rc<RefCell<(Option<String>, Vec<String>)>>;
    let compose_thread: ComposeThread = Rc::new(RefCell::new((None, Vec::new())));
    // the draft being edited (Some = resuming/saving over an existing draft), and the drafts list
    let current_draft_id = Rc::new(RefCell::new(Option::<i64>::None));
    let drafts_model = Rc::new(VecModel::<DraftItem>::default());
    ui.set_drafts(ModelRc::from(drafts_model.clone()));
    // files attached to the message being composed (bytes live here, not in Slint), + a display model
    let compose_attachments = Rc::new(RefCell::new(
        Vec::<geleit_engine::message::Attachment>::new(),
    ));
    let attach_model = Rc::new(VecModel::<SharedString>::default());
    ui.set_c_attachments(ModelRc::from(attach_model.clone()));
    let suggest_model = Rc::new(VecModel::<SharedString>::default());
    ui.set_c_suggestions(ModelRc::from(suggest_model.clone()));
    let cc_suggest_model = Rc::new(VecModel::<SharedString>::default());
    ui.set_c_cc_suggestions(ModelRc::from(cc_suggest_model.clone()));
    // "Move to…" picker: the message awaiting a destination + the candidate folders
    let pending_move_id = Rc::new(RefCell::new(Option::<i32>::None));
    let move_folders_model = Rc::new(VecModel::<SharedString>::default());
    ui.set_move_folders(ModelRc::from(move_folders_model.clone()));
    let manage_folders_model = Rc::new(VecModel::<SharedString>::default());
    ui.set_manage_folders(ModelRc::from(manage_folders_model.clone()));
    // multi-selected message ids for bulk actions (ORG-7)
    let selected_ids = Rc::new(RefCell::new(HashSet::<i32>::new()));
    // the account currently in view (MULTI-1) is the UI `current-account` prop (source of truth)
    let accounts_model = Rc::new(VecModel::<AccountItem>::default());
    ui.set_accounts(ModelRc::from(accounts_model.clone()));
    ui.set_folders(ModelRc::from(folders_model.clone()));
    ui.set_messages(ModelRc::from(messages.clone()));
    // Apply the persisted theme (APP-3) before the first paint.
    ui.global::<Palette>()
        .set_dark(store.get_setting("theme").ok().flatten().as_deref() == Some("dark"));

    // Pump GTK (so the embedded webview renders) under Slint's loop, and keep the webview's bounds
    // on the reading-pane body while it's shown. Kept alive for the app's lifetime.
    let gtk_pump = slint::Timer::default();
    {
        let weak = ui.as_weak();
        let view = html_view.clone();
        gtk_pump.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(16),
            move || {
                if gtk_ready {
                    while gtk::events_pending() {
                        gtk::main_iteration_do(false);
                    }
                }
                if view.visible.get() {
                    if let (Some(ui), Some(w)) = (weak.upgrade(), view.webview.borrow().as_ref()) {
                        let _ = w.set_bounds(body_rect(&ui));
                    }
                }
            },
        );
    }

    // NOTE: the webview is built lazily on first HTML message (in `show_html`/`ensure_webview`), NOT
    // eagerly at startup. Building it at launch raced Slint's GL renderer and crashed the window with
    // an async `GLXBadWindow` before anything was clicked (regression from S3.6). Lazy build keeps the
    // app opening reliably; the proper fix for the GL coexistence is a follow-up (software renderer).

    // full reload (also the initial load) — reused by setup success
    {
        let weak = ui.as_weak();
        let store = store.clone();
        let fm = folders_model.clone();
        let fids = folder_ids.clone();
        let msgs = messages.clone();
        let accts = accounts_model.clone();
        let view = html_view.clone();
        ui.on_reload(move || {
            if let Some(ui) = weak.upgrade() {
                reload_all(&ui, &store, &fm, &fids, &msgs, &accts);
                hide_html(&view); // nothing selected → show the Slint pane / form
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
        let view = html_view.clone();
        let fm = folders_model.clone();
        let selected = selected_ids.clone();
        ui.on_folder_selected(move |idx| {
            let Some(ui) = weak.upgrade() else { return };
            let Some(fid) = fids.borrow().get(idx as usize).copied() else {
                return;
            };
            ui.set_selected_folder(idx);
            ui.set_selected_message(0);
            ui.set_remote_blocked(false);
            selected.borrow_mut().clear(); // a new folder starts with nothing selected
            ui.set_selected_count(0);
            ui.set_nav_index(-1); // keyboard focus restarts in the new list
            ui.set_searching(false); // leaving search to browse a folder
            ui.set_search_query(SharedString::new());
            ui.set_search_count(0);
            // is this the Trash folder? → offer Empty Trash + make Delete permanent (ORG-2)
            let name = fm
                .row_data(idx as usize)
                .map(|s| s.to_string())
                .unwrap_or_default();
            let one = std::slice::from_ref(&name);
            ui.set_viewing_trash(
                viewmodel::find_folder(one, &["trash", "deleted", "bin"]).is_some(),
            );
            ui.set_viewing_junk(viewmodel::find_folder(one, &["junk", "spam"]).is_some());
            model.set_vec(load_messages(&store, fid));
            hide_html(&view); // selection cleared
        });
    }

    // message click → open it in the reading pane and mark it read
    {
        let weak = ui.as_weak();
        let store = store.clone();
        let model = messages.clone();
        let view = html_view.clone();
        let current_allowed = current_allowed.clone();
        let db_path = db.clone();
        let secrets = secrets.clone();
        ui.on_message_selected(move |item| {
            let Some(ui) = weak.upgrade() else { return };
            // A cross-account search hit: switch to its account first, so the rail + actions target
            // the right account (SEARCH-5). reload_all loads that account's view; we open below.
            if item.account != 0 && item.account != ui.get_current_account() {
                ui.set_current_account(item.account);
                ui.set_searching(false);
                ui.set_search_query(SharedString::new());
                ui.invoke_reload();
            }
            ui.set_selected_message(item.id);
            ui.set_r_subject(item.subject.clone());
            ui.set_r_starred(item.starred);
            ui.set_r_sender(item.sender.clone());
            ui.set_r_date(item.date.clone());
            let stored = store.body_for(item.id.into());
            let (body_text, html) = match &stored {
                Ok(b) => (
                    viewmodel::body_display(b.as_ref()),
                    b.as_ref().and_then(|x| x.html.clone()),
                ),
                Err(_) => ("(Could not load this message.)".to_owned(), None),
            };
            ui.set_r_body(body_text.into());
            // HTML messages render in the sandboxed webview; text messages use the Slint pane.
            // Default = remote blocked; if the message had remote content, offer to load it (PRIV-2/3).
            use geleit_engine::safehtml;
            match html {
                Some(h) => {
                    // Compare the sanitized BODIES (not the wrapped docs, whose CSP always differs):
                    // they differ iff the message had remote content → show the cue (PRIV-3).
                    let body_blocked = safehtml::sanitize_html(&h);
                    let body_allowed = safehtml::sanitize_html_allowing_remote(&h);
                    ui.set_remote_blocked(body_blocked != body_allowed);
                    *current_allowed.borrow_mut() = Some(safehtml::document(&body_allowed, true));
                    show_html(&ui, &view, &safehtml::document(&body_blocked, false));
                }
                None => {
                    ui.set_remote_blocked(false);
                    *current_allowed.borrow_mut() = None;
                    hide_html(&view);
                }
            }
            let labels: Vec<SharedString> = store
                .attachments_for(item.id.into())
                .unwrap_or_default()
                .iter()
                .map(|a| viewmodel::attachment_label(a.filename.as_deref(), a.size as u64).into())
                .collect();
            ui.set_r_attachments(ModelRc::new(VecModel::from(labels)));
            if item.unread {
                let _ = store.set_seen(item.id.into(), true);
                flip_unread(&model, item.id, false); // in place — keeps scroll
                                                     // write \Seen back to the server (SYNC-5), using the message's real folder
                if let Ok(Some((folder, uid))) = store.message_location(item.id.into()) {
                    spawn_set_seen(
                        &db_path,
                        &secrets,
                        ui.get_current_account() as i64,
                        folder,
                        uid as u32,
                        true,
                    );
                }
            }
        });
    }

    // "Mark as unread" → flip read state locally + in place, then write \Seen back (SYNC-5)
    {
        let weak = ui.as_weak();
        let store = store.clone();
        let model = messages.clone();
        let db_path = db.clone();
        let secrets = secrets.clone();
        ui.on_mark_unread(move |id| {
            let Some(ui) = weak.upgrade() else { return };
            let _ = store.set_seen(id.into(), false);
            flip_unread(&model, id, true);
            if let Ok(Some((folder, uid))) = store.message_location(id.into()) {
                spawn_set_seen(
                    &db_path,
                    &secrets,
                    ui.get_current_account() as i64,
                    folder,
                    uid as u32,
                    false,
                );
            }
        });
    }

    // Star / unstar (ORG-4): optimistic local flip, then write \Flagged back to the server.
    {
        let weak = ui.as_weak();
        let store = store.clone();
        let model = messages.clone();
        let folders_model = folders_model.clone();
        let db_path = db.clone();
        let secrets = secrets.clone();
        ui.on_toggle_star(move |id| {
            let Some(ui) = weak.upgrade() else { return };
            let Some(header) = store.header_by_id(id.into()).ok().flatten() else {
                return;
            };
            let new_flag = !header.flagged;
            let Ok(uid) = store.set_flagged(id.into(), new_flag) else {
                return;
            };
            ui.set_r_starred(new_flag);
            flip_starred(&model, id, new_flag);
            // server write-back (skip if the message has no UID, e.g. not yet synced)
            let Some(uid) = uid else { return };
            let folder = folders_model
                .row_data(ui.get_selected_folder() as usize)
                .map(|s| s.to_string())
                .unwrap_or_else(|| "INBOX".to_owned());
            let weak = weak.clone();
            let db_path = db_path.clone();
            let secrets = secrets.clone();
            let acct = ui.get_current_account() as i64;
            std::thread::spawn(move || {
                let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    refresh::run_set_flag(&db_path, &*secrets, acct, &folder, uid as u32, new_flag)
                }))
                .unwrap_or_else(|_| Err("Couldn't update the star.".to_owned()));
                if let Err(msg) = res {
                    let w = weak.clone();
                    // the star stays set locally (preserved on re-sync); just note the sync miss
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = w.upgrade() {
                            ui.set_sync_status(msg.into());
                        }
                    });
                }
            });
        });
    }

    // Archive (ORG-1) → move to the Archive folder.
    {
        let weak = ui.as_weak();
        let store = store.clone();
        let messages = messages.clone();
        let fm = folders_model.clone();
        let view = html_view.clone();
        let db_path = db.clone();
        let secrets = secrets.clone();
        ui.on_archive_message(move |id| {
            let Some(ui) = weak.upgrade() else { return };
            let names = folder_names(&fm);
            match viewmodel::find_folder(&names, &["archive"]) {
                Some(target) => perform_move(
                    &ui,
                    &store,
                    &messages,
                    &fm,
                    &view,
                    &db_path,
                    &secrets,
                    ui.get_current_account() as i64,
                    id,
                    target,
                ),
                None => ui.set_status("This account has no Archive folder.".into()),
            }
        });
    }

    // Delete (ORG-2) → move to Trash.
    {
        let weak = ui.as_weak();
        let store = store.clone();
        let messages = messages.clone();
        let fm = folders_model.clone();
        let view = html_view.clone();
        let db_path = db.clone();
        let secrets = secrets.clone();
        ui.on_trash_message(move |id| {
            let Some(ui) = weak.upgrade() else { return };
            // In Trash already → permanent delete; otherwise move to Trash.
            if ui.get_viewing_trash() {
                let folder = fm
                    .row_data(ui.get_selected_folder() as usize)
                    .map(|s| s.to_string())
                    .unwrap_or_default();
                let uid = store
                    .header_by_id(id.into())
                    .ok()
                    .flatten()
                    .and_then(|h| h.uid);
                let _ = store.delete_message(id.into());
                remove_row(&messages, id);
                ui.set_selected_message(0);
                hide_html(&view);
                if let Some(uid) = uid {
                    let weak = weak.clone();
                    let db_path = db_path.clone();
                    let secrets = secrets.clone();
                    let acct = ui.get_current_account() as i64;
                    std::thread::spawn(move || {
                        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            refresh::run_delete_permanently(
                                &db_path, &*secrets, acct, &folder, uid as u32,
                            )
                        }))
                        .unwrap_or_else(|_| Err("Couldn't delete the message.".to_owned()));
                        if let Err(msg) = res {
                            let w = weak.clone();
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(ui) = w.upgrade() {
                                    ui.set_status(msg.into());
                                }
                            });
                        }
                    });
                }
                return;
            }
            let names = folder_names(&fm);
            match viewmodel::find_folder(&names, &["trash", "deleted", "bin"]) {
                Some(target) => perform_move(
                    &ui,
                    &store,
                    &messages,
                    &fm,
                    &view,
                    &db_path,
                    &secrets,
                    ui.get_current_account() as i64,
                    id,
                    target,
                ),
                None => ui.set_status("This account has no Trash folder.".into()),
            }
        });
    }

    // Empty Trash (ORG-2) → clear the local Trash folder + expunge it on the server.
    {
        let weak = ui.as_weak();
        let store = store.clone();
        let messages = messages.clone();
        let fm = folders_model.clone();
        let fids = folder_ids.clone();
        let view = html_view.clone();
        let db_path = db.clone();
        let secrets = secrets.clone();
        ui.on_empty_trash(move || {
            let Some(ui) = weak.upgrade() else { return };
            let idx = ui.get_selected_folder() as usize;
            let folder = fm.row_data(idx).map(|s| s.to_string()).unwrap_or_default();
            if let Some(fid) = fids.borrow().get(idx).copied() {
                let _ = store.clear_folder(fid); // optimistic local empty
            }
            messages.set_vec(Vec::new());
            ui.set_selected_message(0);
            hide_html(&view);
            let weak = weak.clone();
            let db_path = db_path.clone();
            let secrets = secrets.clone();
            let acct = ui.get_current_account() as i64;
            std::thread::spawn(move || {
                let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    refresh::run_empty_folder(&db_path, &*secrets, acct, &folder)
                }))
                .unwrap_or_else(|_| Err("Couldn't empty the Trash on the server.".to_owned()));
                if let Err(msg) = res {
                    let w = weak.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = w.upgrade() {
                            ui.set_status(msg.into());
                        }
                    });
                }
            });
        });
    }

    // Spam / Not spam (ORG-5): move to Junk, or back to Inbox when already in Junk.
    {
        let weak = ui.as_weak();
        let store = store.clone();
        let messages = messages.clone();
        let fm = folders_model.clone();
        let view = html_view.clone();
        let db_path = db.clone();
        let secrets = secrets.clone();
        ui.on_toggle_junk(move |id| {
            let Some(ui) = weak.upgrade() else { return };
            let names = folder_names(&fm);
            if ui.get_viewing_junk() {
                // Not spam → back to Inbox
                match viewmodel::find_folder(&names, &["inbox"]) {
                    Some(target) => perform_move(
                        &ui,
                        &store,
                        &messages,
                        &fm,
                        &view,
                        &db_path,
                        &secrets,
                        ui.get_current_account() as i64,
                        id,
                        target,
                    ),
                    None => ui.set_status("No Inbox folder found.".into()),
                }
            } else {
                match viewmodel::find_folder(&names, &["junk", "spam"]) {
                    Some(target) => perform_move(
                        &ui,
                        &store,
                        &messages,
                        &fm,
                        &view,
                        &db_path,
                        &secrets,
                        ui.get_current_account() as i64,
                        id,
                        target,
                    ),
                    None => ui.set_status("This account has no Junk folder.".into()),
                }
            }
        });
    }

    // Move… (ORG-3) → open the folder picker, then move to the chosen folder.
    {
        let weak = ui.as_weak();
        let fm = folders_model.clone();
        let mfm = move_folders_model.clone();
        let pending = pending_move_id.clone();
        ui.on_open_move(move |id| {
            let Some(ui) = weak.upgrade() else { return };
            *pending.borrow_mut() = Some(id);
            let current = fm
                .row_data(ui.get_selected_folder() as usize)
                .map(|s| s.to_string())
                .unwrap_or_default();
            let others: Vec<SharedString> = folder_names(&fm)
                .into_iter()
                .filter(|f| !f.eq_ignore_ascii_case(&current))
                .map(Into::into)
                .collect();
            mfm.set_vec(others);
            ui.set_picking_folder(true);
        });
    }
    {
        let weak = ui.as_weak();
        let store = store.clone();
        let messages = messages.clone();
        let fm = folders_model.clone();
        let view = html_view.clone();
        let db_path = db.clone();
        let secrets = secrets.clone();
        let pending = pending_move_id.clone();
        ui.on_move_to(move |target| {
            let Some(ui) = weak.upgrade() else { return };
            ui.set_picking_folder(false);
            if let Some(id) = pending.borrow_mut().take() {
                perform_move(
                    &ui,
                    &store,
                    &messages,
                    &fm,
                    &view,
                    &db_path,
                    &secrets,
                    ui.get_current_account() as i64,
                    id,
                    &target,
                );
            }
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_close_move(move || {
            if let Some(ui) = weak.upgrade() {
                ui.set_picking_folder(false);
            }
        });
    }

    // Manage folders (ORG-6): open the manager, create / rename / delete on the server.
    {
        let weak = ui.as_weak();
        let fm = folders_model.clone();
        let mfm = manage_folders_model.clone();
        ui.on_open_manage_folders(move || {
            let Some(ui) = weak.upgrade() else { return };
            let rows: Vec<SharedString> = folder_names(&fm).into_iter().map(Into::into).collect();
            mfm.set_vec(rows);
            ui.set_mf_name(SharedString::new());
            ui.set_managing_folders(true);
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_close_manage_folders(move || {
            if let Some(ui) = weak.upgrade() {
                ui.set_managing_folders(false);
            }
        });
    }
    {
        let weak = ui.as_weak();
        let db_path = db.clone();
        let secrets = secrets.clone();
        ui.on_create_folder(move || {
            let Some(ui) = weak.upgrade() else { return };
            let name = ui.get_mf_name().trim().to_owned();
            if name.is_empty() {
                ui.set_status("Enter a folder name.".into());
                return;
            }
            let (db_path, secrets) = (db_path.clone(), secrets.clone());
            let acct = ui.get_current_account() as i64;
            spawn_folder_op(weak.clone(), move || {
                refresh::run_create_folder(&db_path, &*secrets, acct, &name)
            });
        });
    }
    {
        let weak = ui.as_weak();
        let db_path = db.clone();
        let secrets = secrets.clone();
        ui.on_rename_folder(move |from| {
            let Some(ui) = weak.upgrade() else { return };
            let to = ui.get_mf_name().trim().to_owned();
            if to.is_empty() {
                ui.set_status("Type the new name above, then choose Rename.".into());
                return;
            }
            let (db_path, secrets, from) = (db_path.clone(), secrets.clone(), from.to_string());
            let acct = ui.get_current_account() as i64;
            spawn_folder_op(weak.clone(), move || {
                refresh::run_rename_folder(&db_path, &*secrets, acct, &from, &to)
            });
        });
    }
    {
        let weak = ui.as_weak();
        let db_path = db.clone();
        let secrets = secrets.clone();
        ui.on_delete_folder(move |name| {
            let Some(ui) = weak.upgrade() else { return };
            let (db_path, secrets, name) = (db_path.clone(), secrets.clone(), name.to_string());
            let acct = ui.get_current_account() as i64;
            spawn_folder_op(weak.clone(), move || {
                refresh::run_delete_folder(&db_path, &*secrets, acct, &name)
            });
        });
    }

    // Multi-select + bulk actions (ORG-7).
    {
        let weak = ui.as_weak();
        let model = messages.clone();
        let selected = selected_ids.clone();
        ui.on_toggle_select(move |id| {
            let Some(ui) = weak.upgrade() else { return };
            let now = {
                let mut set = selected.borrow_mut();
                if set.remove(&id) {
                    false
                } else {
                    set.insert(id);
                    true
                }
            };
            flip_selected_row(&model, id, now);
            ui.set_selected_count(selected.borrow().len() as i32);
        });
    }
    {
        let weak = ui.as_weak();
        let model = messages.clone();
        let selected = selected_ids.clone();
        ui.on_clear_selection(move || {
            let Some(ui) = weak.upgrade() else { return };
            for id in selected.borrow().iter() {
                flip_selected_row(&model, *id, false);
            }
            selected.borrow_mut().clear();
            ui.set_selected_count(0);
        });
    }
    {
        let weak = ui.as_weak();
        let store = store.clone();
        let messages = messages.clone();
        let fm = folders_model.clone();
        let view = html_view.clone();
        let db_path = db.clone();
        let secrets = secrets.clone();
        let selected = selected_ids.clone();
        ui.on_bulk_archive(move || {
            let Some(ui) = weak.upgrade() else { return };
            match viewmodel::find_folder(&folder_names(&fm), &["archive"]).map(str::to_owned) {
                Some(target) => bulk_move(
                    &ui,
                    &store,
                    &messages,
                    &fm,
                    &view,
                    &db_path,
                    &secrets,
                    ui.get_current_account() as i64,
                    &selected,
                    &target,
                ),
                None => ui.set_status("This account has no Archive folder.".into()),
            }
        });
    }
    {
        let weak = ui.as_weak();
        let store = store.clone();
        let messages = messages.clone();
        let fm = folders_model.clone();
        let view = html_view.clone();
        let db_path = db.clone();
        let secrets = secrets.clone();
        let selected = selected_ids.clone();
        ui.on_bulk_trash(move || {
            let Some(ui) = weak.upgrade() else { return };
            match viewmodel::find_folder(&folder_names(&fm), &["trash", "deleted", "bin"])
                .map(str::to_owned)
            {
                Some(target) => bulk_move(
                    &ui,
                    &store,
                    &messages,
                    &fm,
                    &view,
                    &db_path,
                    &secrets,
                    ui.get_current_account() as i64,
                    &selected,
                    &target,
                ),
                None => ui.set_status("This account has no Trash folder.".into()),
            }
        });
    }
    {
        let weak = ui.as_weak();
        let store = store.clone();
        let messages = messages.clone();
        let fm = folders_model.clone();
        let db_path = db.clone();
        let secrets = secrets.clone();
        let selected = selected_ids.clone();
        ui.on_bulk_star(move || {
            let Some(ui) = weak.upgrade() else { return };
            bulk_star(
                &ui,
                &store,
                &messages,
                &fm,
                &db_path,
                &secrets,
                ui.get_current_account() as i64,
                &selected,
            );
        });
    }
    // Bulk mark-as-read (ORG-7): local read state + \Seen write-back (SYNC-5) in one worker.
    {
        let weak = ui.as_weak();
        let store = store.clone();
        let messages = messages.clone();
        let selected = selected_ids.clone();
        let db_path = db.clone();
        let secrets = secrets.clone();
        ui.on_bulk_mark_read(move || {
            let Some(ui) = weak.upgrade() else { return };
            let mut locs: Vec<(String, u32)> = Vec::new();
            for id in selected.borrow().iter() {
                let _ = store.set_seen((*id).into(), true);
                flip_unread(&messages, *id, false);
                flip_selected_row(&messages, *id, false);
                if let Ok(Some((folder, uid))) = store.message_location((*id).into()) {
                    locs.push((folder, uid as u32));
                }
            }
            selected.borrow_mut().clear();
            ui.set_selected_count(0);
            if !locs.is_empty() {
                let account_id = ui.get_current_account() as i64;
                let (db_path, secrets) = (db_path.clone(), secrets.clone());
                std::thread::spawn(move || {
                    for (folder, uid) in locs {
                        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            refresh::run_set_seen(
                                &db_path, &*secrets, account_id, &folder, uid, true,
                            )
                        }));
                    }
                });
            }
        });
    }

    // Search (SEARCH-1/2/3): query the local FTS index on each keystroke (offline, instant). Empty
    // query returns to the current folder.
    {
        let weak = ui.as_weak();
        let store = store.clone();
        let model = messages.clone();
        let fids = folder_ids.clone();
        let view = html_view.clone();
        let run_search = move |ui: &Main| {
            let query = ui.get_search_query().trim().to_owned();
            if query.is_empty() {
                ui.set_searching(false);
                ui.set_search_count(0);
                // back to the current folder
                if let Some(fid) = fids
                    .borrow()
                    .get(ui.get_selected_folder() as usize)
                    .copied()
                {
                    model.set_vec(load_messages(&store, fid));
                }
                ui.set_selected_message(0);
                ui.set_nav_index(-1);
                hide_html(&view);
                return;
            }
            let items = search_result_items(
                &store,
                ui.get_current_account() as i64,
                &query,
                ui.get_search_all(),
            );
            ui.set_search_count(items.len() as i32);
            model.set_vec(items);
            ui.set_searching(true);
            ui.set_selected_message(0);
            ui.set_nav_index(-1);
            hide_html(&view);
        };
        let run2 = run_search.clone();
        ui.on_search_edited(move || {
            if let Some(ui) = weak.upgrade() {
                run_search(&ui);
            }
        });
        let weak2 = ui.as_weak();
        ui.on_clear_search(move || {
            if let Some(ui) = weak2.upgrade() {
                ui.set_search_query(SharedString::new());
                run2(&ui);
            }
        });
    }
    // Toggle "all accounts" (SEARCH-5) → re-run the current query in the new scope.
    {
        let weak = ui.as_weak();
        ui.on_toggle_search_all(move || {
            let Some(ui) = weak.upgrade() else { return };
            ui.set_search_all(!ui.get_search_all());
            ui.invoke_search_edited();
        });
    }

    // "Load remote images" → re-render the open message with remote content allowed (PRIV-2 opt-in).
    {
        let weak = ui.as_weak();
        let view = html_view.clone();
        let current_allowed = current_allowed.clone();
        ui.on_load_remote(move || {
            let Some(ui) = weak.upgrade() else { return };
            ui.set_remote_blocked(false); // hide the cue first so body_rect repositions the webview
            if let Some(doc) = current_allowed.borrow().as_ref() {
                show_html(&ui, &view, doc);
            }
        });
    }

    // Compose → open the new-message overlay (hide the webview so it can't cover it).
    {
        let weak = ui.as_weak();
        let view = html_view.clone();
        let thread = compose_thread.clone();
        let store = store.clone();
        let draft_id = current_draft_id.clone();
        let atts = compose_attachments.clone();
        let amodel = attach_model.clone();
        let smodel = suggest_model.clone();
        let csmodel = cc_suggest_model.clone();
        ui.on_compose(move || {
            let Some(ui) = weak.upgrade() else { return };
            hide_html(&view);
            *thread.borrow_mut() = (None, Vec::new()); // a fresh message, not a reply
            *draft_id.borrow_mut() = None; // not editing an existing draft
            atts.borrow_mut().clear();
            refresh_attachments(&amodel, &atts.borrow());
            smodel.set_vec(Vec::<SharedString>::new());
            csmodel.set_vec(Vec::<SharedString>::new());
            ui.set_c_attach_path(SharedString::new());
            ui.set_c_markdown(false);
            ui.set_c_to(SharedString::new());
            ui.set_c_cc(SharedString::new());
            ui.set_c_subject(SharedString::new());
            ui.set_c_body(
                geleit_engine::message::signature_block(&account_signature(&store)).into(),
            );
            ui.set_compose_status(SharedString::new());
            ui.set_composing(true);
        });
    }

    // Reply / Forward → pre-fill the compose overlay from the open message (threading via the engine).
    {
        let weak = ui.as_weak();
        let store = store.clone();
        let view = html_view.clone();
        let thread = compose_thread.clone();
        let draft_id = current_draft_id.clone();
        let atts = compose_attachments.clone();
        let amodel = attach_model.clone();
        let smodel = suggest_model.clone();
        let csmodel = cc_suggest_model.clone();
        // kind: 0 = reply, 1 = reply-all, 2 = forward
        let open_compose = move |kind: u8| {
            let Some(ui) = weak.upgrade() else { return };
            let id: i64 = ui.get_selected_message().into();
            if id == 0 {
                return;
            }
            let Some(header) = store.header_by_id(id).ok().flatten() else {
                return;
            };
            let body = store
                .body_for(id)
                .ok()
                .flatten()
                .and_then(|b| b.plain)
                .unwrap_or_default();
            let date = viewmodel::format_date(header.date);
            let date = (!date.is_empty()).then_some(date);
            let orig = geleit_engine::message::Original {
                from_name: header.from_name.as_deref(),
                from_addr: header.from_addr.as_deref().unwrap_or(""),
                subject: header.subject.as_deref().unwrap_or(""),
                date: date.as_deref(),
                message_id: header.message_id.as_deref(),
                in_reply_to: header.in_reply_to.as_deref(),
                to: header.to_addrs.as_deref().unwrap_or(""),
                cc: header.cc_addrs.as_deref().unwrap_or(""),
                body_text: &body,
            };
            // from_* are set by run_send from the account; we only use to/cc/subject/body + threading
            let my_addrs = [ui.get_account().to_string()];
            let draft = match kind {
                0 => geleit_engine::message::reply(&orig, None, String::new()),
                1 => geleit_engine::message::reply_all(&orig, &my_addrs, None, String::new()),
                _ => geleit_engine::message::forward(&orig, None, String::new()),
            };
            hide_html(&view);
            *draft_id.borrow_mut() = None; // a reply/forward is a new draft
            atts.borrow_mut().clear();
            refresh_attachments(&amodel, &atts.borrow());
            smodel.set_vec(Vec::<SharedString>::new());
            csmodel.set_vec(Vec::<SharedString>::new());
            ui.set_c_attach_path(SharedString::new());
            *thread.borrow_mut() = (draft.in_reply_to.clone(), draft.references.clone());
            ui.set_c_to(draft.to.join(", ").into());
            ui.set_c_cc(draft.cc.join(", ").into());
            ui.set_c_subject(draft.subject.into());
            let body = format!(
                "{}{}",
                draft.body_text,
                geleit_engine::message::signature_block(&account_signature(&store))
            );
            ui.set_c_body(body.into());
            ui.set_compose_status(SharedString::new());
            ui.set_composing(true);
        };
        let reply = open_compose.clone();
        let reply_all = open_compose.clone();
        ui.on_reply(move || reply(0));
        ui.on_reply_all(move || reply_all(1));
        ui.on_forward(move || open_compose(2));
    }
    {
        let weak = ui.as_weak();
        ui.on_cancel_compose(move || {
            if let Some(ui) = weak.upgrade() {
                ui.set_composing(false);
            }
        });
    }

    // Send → build + send on a worker thread (P1), then close the overlay or show a calm error.
    {
        let weak = ui.as_weak();
        let db_path = db.clone();
        let secrets = secrets.clone();
        let thread = compose_thread.clone();
        let draft_id = current_draft_id.clone();
        let atts = compose_attachments.clone();
        ui.on_send_message(move || {
            let Some(ui) = weak.upgrade() else { return };
            if ui.get_sending() {
                return;
            }
            if ui.get_c_to().trim().is_empty() {
                ui.set_compose_status("Add at least one recipient.".into());
                return;
            }
            let (to, cc, subject, body) = (
                ui.get_c_to().to_string(),
                ui.get_c_cc().to_string(),
                ui.get_c_subject().to_string(),
                ui.get_c_body().to_string(),
            );
            let (in_reply_to, references) = thread.borrow().clone();
            let attachments = atts.borrow().clone();
            let markdown = ui.get_c_markdown();
            let draft = *draft_id.borrow();
            ui.set_compose_status(SharedString::new());
            ui.set_sending(true);

            let weak = weak.clone();
            let db_path = db_path.clone();
            let secrets = secrets.clone();
            let acct = ui.get_current_account() as i64;
            std::thread::spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    refresh::run_send(
                        &db_path,
                        &*secrets,
                        acct,
                        &to,
                        &cc,
                        &subject,
                        &body,
                        in_reply_to,
                        references,
                        attachments,
                        markdown,
                        draft,
                    )
                }))
                .unwrap_or_else(|_| Err("Couldn't send — something went wrong.".to_owned()));
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(ui) = weak.upgrade() else { return };
                    ui.set_sending(false);
                    match result {
                        Ok(()) => {
                            ui.set_composing(false);
                            ui.set_sync_status("Message sent.".into());
                        }
                        Err(msg) => ui.set_compose_status(msg.into()),
                    }
                });
            });
        });
    }

    // Drafts (SEND-5): open the list, save the current compose, resume a saved one.
    {
        let weak = ui.as_weak();
        let store = store.clone();
        let model = drafts_model.clone();
        ui.on_open_drafts(move || {
            let Some(ui) = weak.upgrade() else { return };
            model.set_vec(load_draft_items(&store));
            ui.set_viewing_drafts(true);
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_close_drafts(move || {
            if let Some(ui) = weak.upgrade() {
                ui.set_viewing_drafts(false);
            }
        });
    }
    {
        let weak = ui.as_weak();
        let store = store.clone();
        let thread = compose_thread.clone();
        let draft_id = current_draft_id.clone();
        let view = html_view.clone();
        let atts = compose_attachments.clone();
        ui.on_save_draft(move || {
            let Some(ui) = weak.upgrade() else { return };
            // Save under the account in view, falling back to the first if somehow unset.
            let acc_id = ui.get_current_account() as i64;
            let acc_id = if store.account_by_id(acc_id).ok().flatten().is_some() {
                acc_id
            } else {
                match store
                    .list_accounts()
                    .ok()
                    .and_then(|a| a.into_iter().next())
                {
                    Some(a) => a.id,
                    None => return,
                }
            };
            let (irt, refs) = thread.borrow().clone();
            let content = geleit_store::DraftContent {
                to: ui.get_c_to().to_string(),
                cc: ui.get_c_cc().to_string(),
                subject: ui.get_c_subject().to_string(),
                body: ui.get_c_body().to_string(),
                in_reply_to: irt,
                references: refs,
            };
            let existing = *draft_id.borrow();
            match store.save_draft(acc_id, existing, &content) {
                Ok(id) => {
                    // persist the composed attachments alongside the draft (SEND-4/5)
                    let saved: Vec<geleit_store::DraftAttachment> = atts
                        .borrow()
                        .iter()
                        .map(|a| geleit_store::DraftAttachment {
                            filename: (!a.filename.is_empty()).then(|| a.filename.clone()),
                            content_type: a.content_type.clone(),
                            data: a.data.clone(),
                        })
                        .collect();
                    let _ = store.replace_draft_attachments(id, &saved);
                    *draft_id.borrow_mut() = Some(id);
                    ui.set_composing(false);
                    hide_html(&view);
                    ui.set_sync_status("Draft saved.".into());
                }
                Err(_) => ui.set_compose_status("Couldn't save the draft.".into()),
            }
        });
    }
    {
        let weak = ui.as_weak();
        let store = store.clone();
        let thread = compose_thread.clone();
        let draft_id = current_draft_id.clone();
        let view = html_view.clone();
        let atts = compose_attachments.clone();
        let amodel = attach_model.clone();
        let smodel = suggest_model.clone();
        let csmodel = cc_suggest_model.clone();
        ui.on_resume_draft(move |id| {
            let Some(ui) = weak.upgrade() else { return };
            let Some(d) = store.draft_by_id(id.into()).ok().flatten() else {
                return;
            };
            hide_html(&view);
            *draft_id.borrow_mut() = Some(d.id);
            *thread.borrow_mut() = (d.content.in_reply_to.clone(), d.content.references.clone());
            // restore the draft's saved attachments (SEND-4/5)
            *atts.borrow_mut() = store
                .draft_attachments(d.id)
                .unwrap_or_default()
                .into_iter()
                .map(|a| geleit_engine::message::Attachment {
                    filename: a.filename.unwrap_or_default(),
                    content_type: a.content_type,
                    data: a.data,
                })
                .collect();
            refresh_attachments(&amodel, &atts.borrow());
            smodel.set_vec(Vec::<SharedString>::new());
            csmodel.set_vec(Vec::<SharedString>::new());
            ui.set_c_attach_path(SharedString::new());
            ui.set_c_to(d.content.to.into());
            ui.set_c_cc(d.content.cc.into());
            ui.set_c_subject(d.content.subject.into());
            ui.set_c_body(d.content.body.into());
            ui.set_compose_status(SharedString::new());
            ui.set_viewing_drafts(false);
            ui.set_composing(true);
        });
    }

    // Attach a file by path (SEND-4) / remove one. (A native file picker is a follow-up.)
    {
        let weak = ui.as_weak();
        let atts = compose_attachments.clone();
        let amodel = attach_model.clone();
        ui.on_attach_file(move || {
            let Some(ui) = weak.upgrade() else { return };
            let path = ui.get_c_attach_path().to_string();
            let path = path.trim();
            if path.is_empty() {
                return;
            }
            match std::fs::read(path) {
                Ok(data) => {
                    let filename = std::path::Path::new(path)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("attachment")
                        .to_owned();
                    let content_type = geleit_engine::message::guess_content_type(&filename);
                    atts.borrow_mut().push(geleit_engine::message::Attachment {
                        filename,
                        content_type,
                        data,
                    });
                    refresh_attachments(&amodel, &atts.borrow());
                    ui.set_c_attach_path(SharedString::new());
                    ui.set_compose_status(SharedString::new());
                }
                Err(_) => ui.set_compose_status("Couldn't read that file — check the path.".into()),
            }
        });
    }
    {
        let weak = ui.as_weak();
        let atts = compose_attachments.clone();
        let amodel = attach_model.clone();
        ui.on_remove_attachment(move |i| {
            let Some(_ui) = weak.upgrade() else { return };
            let idx = i as usize;
            let mut a = atts.borrow_mut();
            if idx < a.len() {
                a.remove(idx);
            }
            drop(a);
            refresh_attachments(&amodel, &atts.borrow());
        });
    }

    // Browse… → open the native file chooser on a worker (it blocks), then reuse the attach path.
    {
        let weak = ui.as_weak();
        ui.on_browse_file(move || {
            let weak = weak.clone();
            std::thread::spawn(move || {
                if let Some(path) = pick_file_via_dialog() {
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = weak.upgrade() {
                            ui.set_c_attach_path(path.into());
                            ui.invoke_attach_file(); // reuse the read-and-attach path
                        }
                    });
                }
            });
        });
    }

    // Address autocomplete for To (SEND-9): suggest as you type, fill on click.
    {
        let weak = ui.as_weak();
        let store = store.clone();
        let model = suggest_model.clone();
        ui.on_to_edited(move || {
            let Some(ui) = weak.upgrade() else { return };
            let to = ui.get_c_to().to_string();
            let token = viewmodel::last_token(&to);
            model.set_vec(address_suggestions(
                &store,
                ui.get_current_account() as i64,
                token,
            ));
        });
    }
    {
        let weak = ui.as_weak();
        let model = suggest_model.clone();
        ui.on_pick_suggestion(move |addr| {
            let Some(ui) = weak.upgrade() else { return };
            let completed = viewmodel::complete_last_token(&ui.get_c_to(), &addr);
            ui.set_c_to(completed.into());
            model.set_vec(Vec::new());
        });
    }

    // …and the same for Cc.
    {
        let weak = ui.as_weak();
        let store = store.clone();
        let model = cc_suggest_model.clone();
        ui.on_cc_edited(move || {
            let Some(ui) = weak.upgrade() else { return };
            let cc = ui.get_c_cc().to_string();
            let token = viewmodel::last_token(&cc);
            model.set_vec(address_suggestions(
                &store,
                ui.get_current_account() as i64,
                token,
            ));
        });
    }
    {
        let weak = ui.as_weak();
        let model = cc_suggest_model.clone();
        ui.on_pick_cc_suggestion(move |addr| {
            let Some(ui) = weak.upgrade() else { return };
            let completed = viewmodel::complete_last_token(&ui.get_c_cc(), &addr);
            ui.set_c_cc(completed.into());
            model.set_vec(Vec::new());
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
                if let Some(smtp) = store.smtp_settings(account.id).ok().flatten() {
                    ui.set_f_smtp_host(smtp.host.clone().into());
                    ui.set_f_smtp_port(smtp.port.to_string().into());
                    ui.set_f_smtp_starttls(
                        smtp.security == geleit_store::SmtpSecurityKind::StartTls,
                    );
                }
                if let Some(sig) = store.signature(account.id).ok().flatten() {
                    ui.set_f_signature(sig.into());
                }
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
            ui.set_sync_status("Checking for new mail…".into());

            let weak = weak.clone();
            let db_path = db_path.clone();
            let secrets = secrets.clone();
            let acct = ui.get_current_account() as i64;
            std::thread::spawn(move || {
                // Nothing !Send crosses: only `weak` + plain data + the Arc secrets (Send+Sync).
                // Phase 1: incremental sync (recent window) — fast.
                let sync = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    refresh::run_refresh(&db_path, &*secrets, acct, &folder)
                }))
                .unwrap_or_else(|_| Err("Couldn't refresh — something went wrong.".to_owned()));
                if let Err(msg) = sync {
                    let w = weak.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = w.upgrade() {
                            ui.set_refreshing(false);
                            ui.set_sync_status(SharedString::new());
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
                            ui.set_sync_status(format!("Catching up… {n}").into());
                        }
                    });
                };
                let backfill = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    refresh::run_backfill(&db_path, &*secrets, acct, &folder, 200, &mut on_batch)
                }));
                // On failure, keep a *calm* note (not the danger banner) — recent mail is already
                // shown and backfill resumes next refresh.
                let done_note: SharedString = match backfill {
                    Ok(Ok(_)) => SharedString::new(),
                    _ => "Couldn't finish catching up — will resume next refresh.".into(),
                };

                // Done: show the full list, re-enable Refresh, leave the calm note (if any).
                let w = weak.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = w.upgrade() {
                        ui.set_refreshing(false);
                        ui.set_sync_status(done_note);
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
            let smtp = match refresh::build_smtp_settings(
                &ui.get_f_smtp_host(),
                &ui.get_f_smtp_port(),
                ui.get_f_smtp_starttls(),
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
            let signature = ui.get_f_signature().to_string();
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
                        smtp,
                        &signature,
                        &password,
                    )
                }))
                .unwrap_or_else(|_| Err("Couldn't connect — something went wrong.".to_owned()));
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(ui) = weak.upgrade() else { return };
                    ui.set_setup_busy(false);
                    match result {
                        Ok(account_id) => {
                            ui.set_f_pass(SharedString::new());
                            ui.set_setup_error(SharedString::new());
                            ui.set_adding_account(false);
                            ui.set_current_account(account_id as i32); // view the new/updated account
                            ui.invoke_reload(); // show the mail
                        }
                        Err(msg) => ui.set_setup_error(msg.into()),
                    }
                });
            });
        });
    }

    // Remove account → wipe local data + keychain password on a worker, then show the setup form.
    {
        let weak = ui.as_weak();
        let db_path = db.clone();
        let secrets = secrets.clone();
        ui.on_remove_account(move || {
            let Some(ui0) = weak.upgrade() else { return };
            let acct = ui0.get_current_account() as i64;
            let weak = weak.clone();
            let db_path = db_path.clone();
            let secrets = secrets.clone();
            std::thread::spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    refresh::run_remove_account(&db_path, &*secrets, acct)
                }))
                .unwrap_or_else(|_| Err("Couldn't remove the account.".to_owned()));
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(ui) = weak.upgrade() else { return };
                    match result {
                        Ok(password_cleared) => {
                            // fall back to another account (reload picks the first if current is gone)
                            ui.set_current_account(-1);
                            ui.invoke_reload();
                            // surface on the form (the main-view `status` banner is now hidden)
                            ui.set_setup_error(if password_cleared {
                                SharedString::new()
                            } else {
                                "Account removed, but the saved password couldn't be cleared from \
                                 the keychain."
                                    .into()
                            });
                        }
                        Err(msg) => ui.set_status(msg.into()),
                    }
                });
            });
        });
    }

    // Switch the account in view (MULTI-1): show its mail, then sync it.
    {
        let weak = ui.as_weak();
        ui.on_switch_account(move |id| {
            let Some(ui) = weak.upgrade() else { return };
            ui.set_current_account(id);
            ui.invoke_reload();
            ui.invoke_refresh(); // pull fresh mail for the newly-selected account
        });
    }
    // "+ Add account": show a blank setup form without dropping the current account.
    {
        let weak = ui.as_weak();
        ui.on_add_account(move || {
            let Some(ui) = weak.upgrade() else { return };
            for set in [
                Main::set_f_email,
                Main::set_f_host,
                Main::set_f_port,
                Main::set_f_user,
                Main::set_f_pass,
                Main::set_f_name,
                Main::set_f_smtp_host,
                Main::set_f_smtp_port,
                Main::set_f_signature,
                Main::set_setup_error,
            ] {
                set(&ui, SharedString::new());
            }
            ui.set_adding_account(true);
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_cancel_add_account(move || {
            if let Some(ui) = weak.upgrade() {
                ui.set_adding_account(false);
            }
        });
    }

    // Keyboard navigation (READ-9 / APP-6): j/k (or arrows) move + preview; Esc closes overlays.
    {
        let weak = ui.as_weak();
        let model = messages.clone();
        ui.on_nav_next(move || {
            let Some(ui) = weak.upgrade() else { return };
            let i = viewmodel::next_index(ui.get_nav_index(), model.row_count() as i32);
            ui.set_nav_index(i);
            if let Some(item) = (i >= 0).then(|| model.row_data(i as usize)).flatten() {
                ui.invoke_message_selected(item);
            }
        });
    }
    {
        let weak = ui.as_weak();
        let model = messages.clone();
        ui.on_nav_prev(move || {
            let Some(ui) = weak.upgrade() else { return };
            let i = viewmodel::prev_index(ui.get_nav_index(), model.row_count() as i32);
            ui.set_nav_index(i);
            if let Some(item) = (i >= 0).then(|| model.row_data(i as usize)).flatten() {
                ui.invoke_message_selected(item);
            }
        });
    }
    // Settings (APP-3/4): open/close + theme toggle (persisted).
    {
        let weak = ui.as_weak();
        ui.on_open_settings(move || {
            if let Some(ui) = weak.upgrade() {
                ui.set_showing_settings(true);
            }
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_close_settings(move || {
            if let Some(ui) = weak.upgrade() {
                ui.set_showing_settings(false);
            }
        });
    }
    {
        let weak = ui.as_weak();
        let store = store.clone();
        ui.on_toggle_theme(move || {
            let Some(ui) = weak.upgrade() else { return };
            let dark = !ui.global::<Palette>().get_dark();
            ui.global::<Palette>().set_dark(dark);
            let _ = store.set_setting("theme", if dark { "dark" } else { "light" });
        });
    }

    {
        let weak = ui.as_weak();
        ui.on_nav_escape(move || {
            let Some(ui) = weak.upgrade() else { return };
            ui.set_composing(false);
            ui.set_viewing_drafts(false);
            ui.set_picking_folder(false);
            ui.set_managing_folders(false);
            ui.set_adding_account(false);
            ui.set_showing_settings(false);
            if ui.get_searching() {
                ui.invoke_clear_search();
            }
        });
    }

    ui.run()?;
    Ok(())
}
