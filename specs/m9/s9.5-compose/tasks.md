# S9.5 — Tasks

Status: **complete** — gates green; compose overlay verified in-app; mapping unit+mutation-tested.

## Engine
- [x] `run_send` + `parse_addrs` moved into `geleit_engine::sync_actions`; `run_send` re-exported
      (Slint app unchanged, still builds). Message-building + `smtp::send` untouched.

## Shell
- [x] `compose_draft(id, kind)` via pure `dto::compose_draft_from` (reply / reply_all / forward)
- [x] `send_message(...)` — thin worker wrapper over `run_send`
- [x] Tests: reply prefills sender + `Re:` + threading · reply_all drops my address · forward blanks
      recipient + `Fwd:` · unknown kind errors. **Mutation: 34 caught / 0 missed.**

## Frontend
- [x] Compose overlay — the app's own document, a plain form (no iframe)
- [x] New message (rail) · Reply / Reply all / Forward (reading-pane action bar)
- [x] Send on a worker (P1); "Sending…" state; closes on success

## Gates
- [x] fmt · clippy `-D warnings` · tests · deny · wasm · boundary · Slint app builds
- [ ] Code review agent → then merge
- [x] In-app: New-message overlay renders (To/Cc/Subject/Body/Send/Cancel)

## Honest note
No SMTP server is available in this environment, so a **live send** wasn't run here — the send path
reuses `run_send` → `smtp::send`, covered by the engine's in-process SMTP sink test in CI (M4). The
click-driven reply overlay couldn't be screenshot-verified (a dev-hook timing race vs the async
open); the mapping it uses is unit+mutation-tested instead.

## Deferred (named, not smuggled in)
Drafts save/resume UI · attachments-in-compose (needs the native picker) · markdown toggle ·
address autocomplete.
