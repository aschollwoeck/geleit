//! The design's inline-SVG line icon set (1.75px stroke, rounded caps, single `currentColor`).
//! Rendered via `inner_html` so the exact SVG paths from the design handoff are reused verbatim,
//! rather than transcribed into `view!` element syntax. Each is a `<span class="ic">` that inherits
//! `color`, so the icon takes its parent's text color.
use leptos::prelude::*;

/// A line icon by its raw SVG markup (use the `*_SVG` constants below).
pub fn icon(svg: &'static str) -> impl IntoView {
    view! { <span class="ic" inner_html=svg></span> }
}

pub const PLUS: &str = r#"<svg width="16" height="16" viewBox="0 0 16 16" fill="none"><path d="M8 3.2v9.6M3.2 8h9.6" stroke="currentColor" stroke-width="1.75" stroke-linecap="round"/></svg>"#;
pub const CHEVRON_DOWN: &str = r#"<svg width="10" height="10" viewBox="0 0 16 16" fill="none"><path d="m4 6 4 4 4-4" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round"/></svg>"#;
pub const CHEVRON_RIGHT: &str = r#"<svg width="16" height="16" viewBox="0 0 16 16" fill="none"><path d="m6 4 4 4-4 4" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round"/></svg>"#;
pub const COLLAPSE: &str = r#"<svg width="15" height="15" viewBox="0 0 16 16" fill="none"><path d="M3 3v10M12.5 5.5 10 8l2.5 2.5M10 8h3.5" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round"/></svg>"#;
pub const EXPAND: &str = r#"<svg width="15" height="15" viewBox="0 0 16 16" fill="none"><path d="M13 3v10M3.5 5.5 6 8l-2.5 2.5M6 8H2.5" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round"/></svg>"#;
pub const SEARCH: &str = r#"<svg width="16" height="16" viewBox="0 0 16 16" fill="none"><circle cx="7" cy="7" r="4" stroke="currentColor" stroke-width="1.75"/><path d="m10.5 10.5 3 3" stroke="currentColor" stroke-width="1.75" stroke-linecap="round"/></svg>"#;
pub const REFRESH: &str = r#"<svg width="16" height="16" viewBox="0 0 16 16" fill="none"><path d="M13 8a5 5 0 1 1-1.5-3.5M13 2.5v3h-3" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round"/></svg>"#;
pub const THEME: &str = r#"<svg width="15" height="15" viewBox="0 0 16 16" fill="none"><circle cx="8" cy="8" r="3.2" stroke="currentColor" stroke-width="1.75"/><path d="M8 1.8v1.4M8 12.8v1.4M1.8 8h1.4M12.8 8h1.4M3.6 3.6l1 1M11.4 11.4l1 1M12.4 3.6l-1 1M4.6 11.4l-1 1" stroke="currentColor" stroke-width="1.75" stroke-linecap="round"/></svg>"#;
pub const SETTINGS: &str = r#"<svg width="16" height="16" viewBox="0 0 16 16" fill="none"><circle cx="8" cy="8" r="2.3" stroke="currentColor" stroke-width="1.6"/><path d="M8 1.6v1.6M8 12.8v1.6M14.4 8h-1.6M3.2 8H1.6M12.5 3.5l-1.1 1.1M4.6 11.4l-1.1 1.1M12.5 12.5l-1.1-1.1M4.6 4.6 3.5 3.5" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/></svg>"#;
pub const CLOSE: &str = r#"<svg width="13" height="13" viewBox="0 0 16 16" fill="none"><path d="m4 4 8 8M12 4l-8 8" stroke="currentColor" stroke-width="1.75" stroke-linecap="round"/></svg>"#;
pub const BACK: &str = r#"<svg width="15" height="15" viewBox="0 0 16 16" fill="none"><path d="M9.5 4 5.5 8l4 4" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round"/></svg>"#;
pub const CLIP: &str = r#"<svg width="13" height="13" viewBox="0 0 16 16" fill="none"><path d="M11 5.5 6.5 10a1.8 1.8 0 0 0 2.5 2.5l4.5-4.5a3.2 3.2 0 0 0-4.5-4.5L4.5 8a4.6 4.6 0 0 0 6.5 6.5" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/></svg>"#;
pub const WARN: &str = r#"<svg width="15" height="15" viewBox="0 0 16 16" fill="none"><path d="M8 2.8 1.8 13.2h12.4L8 2.8Z" stroke="currentColor" stroke-width="1.75" stroke-linejoin="round"/><path d="M8 7v2.6M8 11.6v.2" stroke="currentColor" stroke-width="1.75" stroke-linecap="round"/></svg>"#;
pub const CHECK: &str = r#"<svg width="15" height="15" viewBox="0 0 16 16" fill="none"><path d="m3 8.5 3 3 7-7" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round"/></svg>"#;
pub const SHIELD: &str = r#"<svg width="14" height="14" viewBox="0 0 16 16" fill="none"><path d="M8 2 3 4v3.5c0 3 2.2 5 5 6.5 2.8-1.5 5-3.5 5-6.5V4L8 2Z" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round"/></svg>"#;

// action-row icons
pub const REPLY: &str = r#"<svg width="15" height="15" viewBox="0 0 16 16" fill="none"><path d="M6.5 4 3 7.5 6.5 11M3 7.5h6a4 4 0 0 1 4 4v1" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round"/></svg>"#;
pub const REPLY_ALL: &str = r#"<svg width="15" height="15" viewBox="0 0 16 16" fill="none"><path d="M5.5 4 2 7.5 5.5 11M8.5 4 5 7.5 8.5 11M5 7.5h5a4 4 0 0 1 4 4v1" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round"/></svg>"#;
pub const FORWARD: &str = r#"<svg width="15" height="15" viewBox="0 0 16 16" fill="none"><path d="M9.5 4 13 7.5 9.5 11M13 7.5H7a4 4 0 0 0-4 4v1" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round"/></svg>"#;
pub const MOVE: &str = r#"<svg width="15" height="15" viewBox="0 0 16 16" fill="none"><path d="M2.5 4.5a1 1 0 0 1 1-1h3l1.5 1.5h4.5a1 1 0 0 1 1 1v6a1 1 0 0 1-1 1h-9a1 1 0 0 1-1-1v-7.5Z" stroke="currentColor" stroke-width="1.75" stroke-linejoin="round"/></svg>"#;
pub const TRASH: &str = r#"<svg width="15" height="15" viewBox="0 0 16 16" fill="none"><path d="M3 4.5h10M6.5 4.5V3.5a1 1 0 0 1 1-1h1a1 1 0 0 1 1 1v1M4.5 4.5 5 12.5a1 1 0 0 0 1 1h4a1 1 0 0 0 1-1l.5-8" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round"/></svg>"#;
pub const UNREAD: &str = r#"<svg width="15" height="15" viewBox="0 0 16 16" fill="none"><path d="M2.5 4.5h11a1 1 0 0 1 1 1v5a1 1 0 0 1-1 1h-11a1 1 0 0 1-1-1v-5a1 1 0 0 1 1-1Z" stroke="currentColor" stroke-width="1.5"/><path d="m2.5 5.5 5.5 4 5.5-4" stroke="currentColor" stroke-width="1.5"/></svg>"#;
/// Outline star (not flagged).
pub const STAR: &str = r#"<svg width="15" height="15" viewBox="0 0 16 16" fill="none"><path d="M8 1.6l1.86 3.77 4.16.6-3.01 2.94.71 4.14L8 11.2 4.28 13.05l.71-4.14L1.98 5.97l4.16-.6L8 1.6Z" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round"/></svg>"#;
/// Filled star (flagged).
pub const STAR_FILLED: &str = r#"<svg width="15" height="15" viewBox="0 0 16 16" fill="currentColor"><path d="M8 1.6l1.86 3.77 4.16.6-3.01 2.94.71 4.14L8 11.2 4.28 13.05l.71-4.14L1.98 5.97l4.16-.6L8 1.6Z"/></svg>"#;
/// The Markdown mark (rounded rect with an "M" and a down chevron).
pub const MARKDOWN: &str = r#"<svg width="15" height="15" viewBox="0 0 16 16" fill="none"><rect x="1" y="3" width="14" height="10" rx="1.6" stroke="currentColor" stroke-width="1.3"/><path d="M3.4 10.6V5.6l1.8 2.1 1.8-2.1v5" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round"/><path d="M10.9 5.6v5m0 0L9.5 9.1m1.4 1.5 1.4-1.5" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round"/></svg>"#;

/// Folder icons keyed by a role/name; unknown folders get the generic folder glyph.
pub fn folder_icon(name: &str, role: Option<&str>) -> &'static str {
    // The role the server gave it, when it gave one: `Papierkorb` is a bin, and the rail should draw it
    // as one even though nothing in its name says so.
    match role.unwrap_or("") {
        "inbox" => return INBOX,
        "sent" => return SENT,
        "archive" => return ARCHIVE,
        "trash" => return TRASH,
        "junk" => return JUNK,
        "drafts" => return DRAFTS,
        _ => {}
    }
    match name.to_ascii_lowercase().as_str() {
        "inbox" => INBOX,
        "sent" => SENT,
        "archive" => ARCHIVE,
        "trash" | "deleted" => TRASH,
        "spam" | "junk" => JUNK,
        "drafts" => DRAFTS,
        _ => FOLDER,
    }
}

pub const INBOX: &str = r#"<svg width="16" height="16" viewBox="0 0 16 16" fill="none"><path d="M2.5 9.5h3.2l1 1.5h2.6l1-1.5h3.2" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round"/><path d="M3.5 3.5h9a1 1 0 0 1 1 1v7a1 1 0 0 1-1 1h-9a1 1 0 0 1-1-1v-7a1 1 0 0 1 1-1Z" stroke="currentColor" stroke-width="1.75" stroke-linejoin="round"/></svg>"#;
pub const SENT: &str = r#"<svg width="16" height="16" viewBox="0 0 16 16" fill="none"><path d="M13.5 2.5 7 9M13.5 2.5 9.5 13l-2.5-4-4-2.5 10.5-4Z" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round"/></svg>"#;
pub const ARCHIVE: &str = r#"<svg width="16" height="16" viewBox="0 0 16 16" fill="none"><rect x="2.5" y="3" width="11" height="3.5" rx="1" stroke="currentColor" stroke-width="1.75"/><path d="M3.5 6.5V12a1 1 0 0 0 1 1h7a1 1 0 0 0 1-1V6.5M6.5 9.5h3" stroke="currentColor" stroke-width="1.75" stroke-linecap="round"/></svg>"#;
pub const JUNK: &str = r#"<svg width="16" height="16" viewBox="0 0 16 16" fill="none"><circle cx="8" cy="8" r="5.2" stroke="currentColor" stroke-width="1.75"/><path d="M4.5 4.5l7 7" stroke="currentColor" stroke-width="1.75" stroke-linecap="round"/></svg>"#;
pub const DRAFTS: &str = r#"<svg width="16" height="16" viewBox="0 0 16 16" fill="none"><path d="M9.5 2.5H4a1 1 0 0 0-1 1v9a1 1 0 0 0 1 1h8a1 1 0 0 0 1-1V6l-3.5-3.5Z" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round"/><path d="M9.5 2.5V6H13" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round"/></svg>"#;
pub const FOLDER: &str = r#"<svg width="16" height="16" viewBox="0 0 16 16" fill="none"><path d="M2.5 4.5a1 1 0 0 1 1-1h3l1.5 1.5h4.5a1 1 0 0 1 1 1v6a1 1 0 0 1-1 1h-9a1 1 0 0 1-1-1v-7.5Z" stroke="currentColor" stroke-width="1.75" stroke-linejoin="round"/></svg>"#;
/// Save to disk (down arrow into a tray) — the reading-pane "Save" (.eml export) action.
pub const DOWNLOAD: &str = r#"<svg width="15" height="15" viewBox="0 0 16 16" fill="none"><path d="M8 2.5v7m0 0L5.2 6.9M8 9.5l2.8-2.6" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/><path d="M3 11.5v1a1 1 0 0 0 1 1h8a1 1 0 0 0 1-1v-1" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/></svg>"#;
/// Open envelope (flap down) — the bulk "Mark read" action, paired with UNREAD (a sealed envelope).
pub const MAILOPEN: &str = r#"<svg width="15" height="15" viewBox="0 0 16 16" fill="none"><path d="M2.5 6.5 8 3l5.5 3.5v5a1 1 0 0 1-1 1h-9a1 1 0 0 1-1-1v-5Z" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round"/><path d="m2.5 6.5 5.5 3.5 5.5-3.5" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/></svg>"#;
/// Horizontal three-dot "more options" glyph — the per-folder Rename/Delete menu trigger.
pub const MORE: &str = r#"<svg width="16" height="16" viewBox="0 0 16 16" fill="currentColor"><circle cx="4" cy="8" r="1.4"/><circle cx="8" cy="8" r="1.4"/><circle cx="12" cy="8" r="1.4"/></svg>"#;
/// Open a file from disk (document with an up arrow) — the "Open mail file…" rail action.
pub const OPENFILE: &str = r#"<svg width="16" height="16" viewBox="0 0 16 16" fill="none"><path d="M9 2.5H4.5a1 1 0 0 0-1 1v9a1 1 0 0 0 1 1h7a1 1 0 0 0 1-1V6" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round"/><path d="M8 11V6.5m0 0L6.3 8.2M8 6.5l1.7 1.7" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/></svg>"#;

/// Settings category icons.
pub const IC_ACCOUNTS: &str = r#"<svg width="16" height="16" viewBox="0 0 16 16" fill="none"><circle cx="8" cy="5.5" r="2.5" stroke="currentColor" stroke-width="1.6"/><path d="M3.5 13c0-2.2 2-3.5 4.5-3.5s4.5 1.3 4.5 3.5" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/></svg>"#;
pub const IC_GENERAL: &str = r#"<svg width="16" height="16" viewBox="0 0 16 16" fill="none"><path d="M2.5 4h11M2.5 8h11M2.5 12h7" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/></svg>"#;
pub const IC_PRIVACY: &str = r#"<svg width="16" height="16" viewBox="0 0 16 16" fill="none"><path d="M8 2 3 4v3.5c0 3 2.2 5 5 6.5 2.8-1.5 5-3.5 5-6.5V4L8 2Z" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round"/></svg>"#;
pub const IC_BELL: &str = r#"<svg width="16" height="16" viewBox="0 0 16 16" fill="none"><path d="M4 7a4 4 0 0 1 8 0c0 3 1 4 1 4H3s1-1 1-4Z" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round"/><path d="M6.5 13a1.5 1.5 0 0 0 3 0" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/></svg>"#;
