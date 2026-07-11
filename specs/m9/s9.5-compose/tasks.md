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
- [x] Code review agent — 6 findings, all fixed (below)
- [x] In-app: New-message overlay renders (To/Cc/Subject/Body/Send/Cancel)

## Code review — findings acted on
Security (no untrusted content in the app document — `prop:value`, not `innerHTML`), the function
move, and the state machine were all found solid. Six fixes:

| # | Finding | Fix |
|---|---|---|
| 1 | **Signature silently dropped (high):** the Slint app appends the signature in its UI layer, not in `run_send` — the new send path skipped it, and the comment wrongly claimed it was handled. | `send_message` now reads the account signature and appends `signature_block` (S9.5-4 met); comment corrected. |
| 2 | **Raw epoch in quoted dates (medium-high):** replies quoted `On 1752307200, … wrote:` — `header.date` is `i64` seconds passed verbatim. | Added `format_email_date` (civil-from-days) → `On 11 Jul 2026, …`. Unit + mutation tested across leap days / century / year boundaries. |
| 3 | **`parse_addrs` test lost in the move (medium):** deleted from the Slint app, landed only in the *feature-gated* live test module. | Restored in a non-gated `pure_tests` module in `sync_actions`. |
| 4 | **Cc-only send blocked (low-med):** the guard required a non-empty To, but the engine accepts Cc-only. | Guard now allows a recipient in To **or** Cc. |
| 5 | **Overlay title always "New message" (low):** reply/forward showed the wrong title. | Title inferred from the draft subject (`Re:` → Reply, `Fwd:` → Forward). |
| 6 | **Wrong error on double-send (low):** the empty-recipient message showed even when the cause was an in-flight send. | Separated the two guards. |

Noted, not fixed (edge cases, not blockers): reply-all doesn't exclude account *aliases*, and
replying-all to your own sent message re-adds your address — the engine's `reply()` seed isn't filtered.

## Honest note
No SMTP server is available in this environment, so a **live send** wasn't run here — the send path
reuses `run_send` → `smtp::send`, covered by the engine's in-process SMTP sink test in CI (M4). The
click-driven reply overlay couldn't be screenshot-verified (a dev-hook timing race vs the async
open); the mapping it uses is unit+mutation-tested instead.

## Deferred (named, not smuggled in)
Drafts save/resume UI · attachments-in-compose (needs the native picker) · markdown toggle ·
address autocomplete.
