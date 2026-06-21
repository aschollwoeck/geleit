# S4.8 — Save sent mail to Sent (SEND-8) · Spec (the WHAT)

Slice of **M4 (Send)**. Type: engine + orchestration. After a message is sent, save a copy to the
account's **Sent** folder via IMAP `APPEND` (marked `\Seen`), so sent mail appears in Sent.

Status: **draft.**

## In scope
- `engine::imap::append_message(config, secrets, folder, bytes)` — IMAP `APPEND` of the full RFC 5322
  bytes, flagged `\Seen`.
- `run_send`: after `smtp::send` succeeds, find the Sent folder (by name) and append a copy.
  **Best-effort** — a failed Sent-save never reports send failure (the message *was* sent).

## Out of scope
- SPECIAL-USE (`\Sent`) folder detection — name heuristic for now (exact "Sent" or contains "sent").
- Creating a Sent folder if absent. Outbox/queue + retry (later).

## Acceptance criteria (measurable)
1. build/test/`clippy -D warnings` (incl. `--features dangerous-tls`)/`fmt`/`cargo deny check` green.
2. `append_message` performs an IMAP APPEND (verified by a live `#[ignore]` test against Dovecot —
   maintainer runs it; not in CI, like the other live IMAP tests).
3. `run_send` saves to Sent on success and does NOT fail the send if the Sent-save fails.

## Deliverables
- `imap::append_message` + live test; Sent-folder lookup + best-effort append in `run_send`.
