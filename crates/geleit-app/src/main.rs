//! `geleit-app` — the Slint shell (S1.7). Renders the local store's folders and a virtualized
//! message list in the "Soft daylight" design (`design.md`). Reads the store only — no network on
//! the UI path (constitution P1); sync is the engine's job, wired in later (S1.9).

mod refresh;
mod viewmodel;

use std::cell::{Cell, RefCell};
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
    import { ListView, ScrollView, LineEdit, TextEdit } from "std-widgets.slint";

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
        thread-count: int, // messages in this conversation (1 = not threaded)
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
        in property <[string]> r-attachments;
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
        // add-account form
        in property <bool> needs-setup;
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
        callback refresh();
        callback connect();
        callback reload();
        callback remove-account();
        callback load-remote();
        callback compose();
        callback send-message();
        callback cancel-compose();
        callback reply();
        callback forward();

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
                            text: "Forward";
                            color: Palette.accent-strong;
                            font-size: 13px;
                            font-weight: 600;
                            TouchArea { clicked => { root.forward(); } }
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
                            Text { text: "SMTP server (for sending)"; color: Palette.muted; font-size: 12px; }
                            LineEdit { placeholder-text: "smtp.example.com"; text <=> root.f-smtp-host; }
                            Text { text: "SMTP port"; color: Palette.muted; font-size: 12px; }
                            LineEdit { placeholder-text: root.f-smtp-starttls ? "587" : "465"; text <=> root.f-smtp-port; }
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
                            Rectangle {
                                height: 72px;
                                border-radius: 6px;
                                border-width: 1px;
                                border-color: Palette.divider;
                                TextEdit { text <=> root.f-signature; wrap: word-wrap; }
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
                    LineEdit { placeholder-text: "name@example.com, …"; text <=> root.c-to; }
                    Text { text: "Cc (optional)"; color: Palette.muted; font-size: 12px; }
                    LineEdit { placeholder-text: "name@example.com, …"; text <=> root.c-cc; }
                    Text { text: "Subject"; color: Palette.muted; font-size: 12px; }
                    LineEdit { text <=> root.c-subject; }
                    TextEdit {
                        text <=> root.c-body;
                        vertical-stretch: 1;
                        wrap: word-wrap;
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
    }
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
    ui.set_sync_status(SharedString::new());
    ui.set_remote_blocked(false);
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
    ui.set_folders(ModelRc::from(folders_model.clone()));
    ui.set_messages(ModelRc::from(messages.clone()));

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

    // Build the webview once, just after the window is up, so the first mail open is instant.
    let webview_init = slint::Timer::default();
    {
        let weak = ui.as_weak();
        let view = html_view.clone();
        webview_init.start(
            slint::TimerMode::SingleShot,
            Duration::from_millis(150),
            move || {
                if let Some(ui) = weak.upgrade() {
                    ensure_webview(&ui, &view);
                }
            },
        );
    }

    // full reload (also the initial load) — reused by setup success
    {
        let weak = ui.as_weak();
        let store = store.clone();
        let fm = folders_model.clone();
        let fids = folder_ids.clone();
        let msgs = messages.clone();
        let view = html_view.clone();
        ui.on_reload(move || {
            if let Some(ui) = weak.upgrade() {
                reload_all(&ui, &store, &fm, &fids, &msgs);
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
        ui.on_folder_selected(move |idx| {
            let Some(ui) = weak.upgrade() else { return };
            let Some(fid) = fids.borrow().get(idx as usize).copied() else {
                return;
            };
            ui.set_selected_folder(idx);
            ui.set_selected_message(0);
            ui.set_remote_blocked(false);
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
        ui.on_message_selected(move |item| {
            let Some(ui) = weak.upgrade() else { return };
            ui.set_selected_message(item.id);
            ui.set_r_subject(item.subject.clone());
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
        ui.on_compose(move || {
            let Some(ui) = weak.upgrade() else { return };
            hide_html(&view);
            *thread.borrow_mut() = (None, Vec::new()); // a fresh message, not a reply
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
        let open_compose = move |is_reply: bool| {
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
                body_text: &body,
            };
            // from_* are set by run_send from the account; we only use to/subject/body + threading
            let draft = if is_reply {
                geleit_engine::message::reply(&orig, None, String::new())
            } else {
                geleit_engine::message::forward(&orig, None, String::new())
            };
            hide_html(&view);
            *thread.borrow_mut() = (draft.in_reply_to.clone(), draft.references.clone());
            ui.set_c_to(draft.to.join(", ").into());
            ui.set_c_cc(SharedString::new());
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
        ui.on_reply(move || reply(true));
        ui.on_forward(move || open_compose(false));
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
            ui.set_compose_status(SharedString::new());
            ui.set_sending(true);

            let weak = weak.clone();
            let db_path = db_path.clone();
            let secrets = secrets.clone();
            std::thread::spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    refresh::run_send(
                        &db_path,
                        &*secrets,
                        &to,
                        &cc,
                        &subject,
                        &body,
                        in_reply_to,
                        references,
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
                    refresh::run_backfill(&db_path, &*secrets, &folder, 200, &mut on_batch)
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

    // Remove account → wipe local data + keychain password on a worker, then show the setup form.
    {
        let weak = ui.as_weak();
        let db_path = db.clone();
        let secrets = secrets.clone();
        ui.on_remove_account(move || {
            let weak = weak.clone();
            let db_path = db_path.clone();
            let secrets = secrets.clone();
            std::thread::spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    refresh::run_remove_account(&db_path, &*secrets)
                }))
                .unwrap_or_else(|_| Err("Couldn't remove the account.".to_owned()));
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(ui) = weak.upgrade() else { return };
                    match result {
                        Ok(password_cleared) => {
                            ui.invoke_reload(); // no account → Add-account form
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

    ui.run()?;
    Ok(())
}
