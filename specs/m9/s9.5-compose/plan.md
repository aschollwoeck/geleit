# S9.5 — Plan

## Engine
Move `run_send` + `parse_addrs` (with its test) from the Slint `refresh.rs` into
`geleit_engine::sync_actions`; re-export `run_send` (S9.3/S9.4 pattern). The message-building
(`message::{reply,reply_all,forward,build}`) and `smtp::send` are untouched — already tested (M4,
incl. the in-process SMTP sink test that runs in CI).

## Shell
- `compose_draft(id, kind)` → builds a prefilled `ComposeDraft` via the pure `dto::compose_draft_from`
  (maps a stored header/body → engine `Original` → reply/reply_all/forward → the form DTO).
- `send_message(...)` → a thin wrapper over `run_send` on a worker (P1); attachments/markdown/draft-id
  are passed empty/false/None (named follow-ups).

## Frontend
A compose overlay — the app's **own document**, a plain form (no iframe: no untrusted content).
New message in the rail; Reply / Reply all / Forward in the reading-pane action bar. Send on a worker.

## Tests
`compose_draft_from` unit- + mutation-tested (reply prefills sender + Re:, reply_all drops my
address, forward blanks the recipient + Fwd:, unknown kind errors). Send reuses the engine's
CI-tested send path. No live SMTP server here — same boundary as M4.
