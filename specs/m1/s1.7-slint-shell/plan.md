# S1.7 — Minimal Slint shell · Plan (the HOW)

Implements `spec.md`. Slint UI built to `design.md`; reads `geleit-store` only.

## `geleit-app::viewmodel` (pure, mutation-tested)
- `MessageVm { sender, subject, snippet, date, unread, attachment }` and
  `message_vm(&MessageHeader) -> MessageVm`: sender = `from_name` else `from_addr` else
  "(unknown sender)"; subject else "(no subject)"; snippet else ""; `unread = !seen`;
  `attachment = has_attachments`; `date = format_date(date)`.
- `format_date(Option<i64>) -> String`: `chrono::DateTime::from_timestamp` → `"%b %e, %H:%M"`
  (deterministic, no "now" → testable); `""` if absent. (Relative "today/yesterday" is later polish.)

## `geleit-app` UI (inline `slint!`)
- A `Palette` global with the `design.md` Soft-daylight tokens (bg, surface, surface-reading, text,
  muted, accent, accent-strong, accent-quiet, divider, danger-strong, radii).
- Three regions: left rail (account label + folder list, Compose button placeholder), centre
  virtualized `ListView` of `MessageItem { sender, subject, snippet, date, unread, attachment }`
  with the row design (unread dot + bold sender, time right, muted snippet, paperclip), selected =
  accent-quiet + 3px accent **guide edge**; right reading pane on `surface-reading` with the guide
  edge — placeholder text for S1.8.
- `callback folder-selected(int)`; root `in property` models for folders + messages.

## `main.rs`
- Open `Store` (path from `GELEIT_DB` env / arg). Load first account → folders; load selected
  folder's `messages_in_folder` (newest-first) → `MessageVm` → `MessageItem` Slint model.
- Wire `folder-selected` → reload the messages model from the store. **No network.**

## Verification
- Unit (CI): `viewmodel` mapping + `format_date`.
- Visual: seed a store (sample account/folders/messages incl. unread + attachment), run the app
  against it on `:0`, screenshot for the PR. (GUI, not in CI.)
- mutants: add `geleit-app` to the package set; exclude `main.rs` (the `slint!` UI / wiring) in
  `.cargo/mutants.toml`; `viewmodel.rs` stays mutation-tested.

## Verify
`cargo build/test --workspace`, `clippy -D warnings`, `fmt`, `cargo deny check`, `cargo mutants`,
plus a manual run + screenshot.
