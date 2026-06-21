# S4.4 — Compose window + send (SEND-1) · Spec (the WHAT)

Slice of **M4 (Send)**. Type: UI + orchestration. Wires the verified foundation (S4.1 transport,
S4.2 message building, S4.3 SMTP settings) into a usable **new-message** flow: a compose window the
person fills in and sends (SEND-1).

Status: **draft.**

## In scope
- App: a **"New message"** button (left rail) + a compose overlay (To, Cc, Subject, multi-line Body,
  Send/Cancel, inline status). The webview is hidden while composing so it can't cover the overlay.
- `refresh::run_send(db, secrets, to, cc, subject, body)` (worker thread, P1): loads the account +
  SMTP settings + the IMAP-shared password, builds the message (S4.2) and sends it (S4.1). Calm,
  PII-free errors. `parse_addrs` (comma/semicolon split) is pure + unit-tested.
- `imap::password` getter so SMTP can reuse the stored credential.

## Out of scope
- Reply/reply-all/forward (S4.3 roadmap → next). Attachments (S4.4 roadmap). Drafts (S4.5). HTML
  body / rich formatting (S4.6). Sent-folder APPEND (later — the server's own copy may suffice for
  some providers; explicit APPEND is a follow-up). Address autocomplete (S4.8).

## Acceptance criteria (measurable)
1. build/test/`clippy -D warnings`/`fmt`/`cargo deny check` green.
2. `parse_addrs` splits/trims/drops-empties — tested. (smtp::send + message::build remain CI-tested
   from S4.1/S4.2; run_send is worker glue, like run_setup — exercised live.)
3. Compose opens, validates "≥1 recipient", sends on a worker (UI never blocks), shows a calm error
   on failure and closes with a "Message sent." note on success — **maintainer eyeballs + sends a
   real message** (the live boundary).
4. P1/P2: send is off the UI thread; password/addresses never logged.

## Deliverables
- Compose overlay + button; `run_send` + `parse_addrs` + `imap::password`; tests; manual update.
