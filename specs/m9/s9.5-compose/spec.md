# S9.5 — Compose, reply, forward, send

**Milestone:** M9. **Constitution:** P1 (send off the UI thread), P3 (calm), P6 (don't lose a draft
mid-send), P2 (the compose window is our own document — never a webview for untrusted content).

## What it delivers

Writing mail: a compose window for **New message**, **Reply**, **Reply all**, and **Forward**, sent
through the account's SMTP server — reusing the engine's message-building and send path unchanged.

| | Story | Acceptance |
|---|---|---|
| **S9.5-1** | I can write and send a new message. | New message → To / Cc / Subject / Body → Send goes out via SMTP; a copy is saved to Sent (best-effort). |
| **S9.5-2** | I can reply / reply all / forward. | From an open message, Reply / Reply all / Forward opens compose **pre-filled** (recipients, `Re:`/`Fwd:` subject, quoted body, threading headers) from the engine's `reply`/`reply_all`/`forward`. |
| **S9.5-3** | Sending never freezes the app. | Send runs on a worker; the UI stays responsive; a calm result (sent / a PII-free error). |
| **S9.5-4** | My signature is appended. | The account signature is included, as today (handled inside the send path). |

## How

- **Reuse:** `message::{reply, reply_all, forward, build, recipients}` and the send path
  (`run_send`) already exist (M4). `run_send` + `parse_addrs` move from the Slint `refresh.rs` into
  the engine (`sync_actions`), re-exported so the Slint app is unchanged — the established pattern.
- **Shell commands:** `reply_draft(id, all)` / `forward_draft(id)` build a prefilled `ComposeDraft`
  DTO from the stored message; `send_message(...)` sends. Draft-building is pure over stored data.
- **Frontend:** a compose overlay (our own document — plain form, no iframe; untrusted content never
  enters compose). New message in the rail; Reply / Reply all / Forward in the reading-pane action bar.

## Out of scope (named follow-ups, not smuggled in)

- **Drafts** (save/resume) — `run_send` already deletes a sent draft by id, and the store's draft
  table exists; the *compose-side save/resume UI* is a follow-up.
- **Attachments in compose** — the send path accepts them; the file-picker UI is a follow-up (needs
  the native picker, like the Slint app's zenity/kdialog path).
- **Markdown formatting toggle**, address autocomplete — follow-ups.

These are deferred deliberately: the core write-and-send path is the slice; the UI-heavy,
least-machine-verifiable extras layer on after.
