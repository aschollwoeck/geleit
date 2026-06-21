# S5.1 — Star / flag (ORG-4) · Spec

First slice of **M5 (Organize)**. Star a message with optimistic UI and write `\Flagged` back to the
server. Establishes the action+write-back pattern the rest of M5 follows.

## In scope
- Store: `message.flagged` captured on sync (first insert; **preserved** on envelope re-sync so a
  local star isn't clobbered); `MessageHeader.flagged`; `set_flagged(id, bool) -> Option<uid>`.
- Engine: `imap::set_flag(config, secrets, folder, uid, flagged)` → `UID STORE +/-FLAGS (\Flagged)`.
- App: a ★/☆ toggle in the reading pane + a ★ marker on starred list rows; toggling flips locally
  (optimistic) and writes back on a worker; a failed write-back leaves the local star + a calm note.

## Out of scope
- Server→local flag changes after first sync (cross-client); a starred/Flagged smart folder.

## Acceptance criteria
1. build/test/clippy -D warnings (+dangerous-tls)/fmt/`cargo deny check` green.
2. Store: flagged synced on insert, preserved on re-sync, settable; exposed by listing + header — tested.
   `message_vm.starred` maps `flagged` — tested.
3. App: ★ toggles optimistically + writes back; failure keeps the star + notes it (maintainer eyeballs).
4. `cargo mutants` — store + viewmodel 0-missed (engine `set_flag` is live-tested glue).

## Deliverables
- Store flagged + set_flagged + tests; `imap::set_flag` + `refresh::run_set_flag`; star UI + handler.
