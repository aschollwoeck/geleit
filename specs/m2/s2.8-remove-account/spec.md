# S2.8 — Remove account (wipe) + offline reading · Spec (the WHAT)

Slice of **M2**. Type: app + engine glue. Delivers **SEC-3** (remove an account → its local data is
wiped) and verifies **OFF-1** (synced mail is readable offline). A privacy capability: a person can
make GeleitMail forget an account entirely on this device.

Status: **draft.**

## Purpose
A **Remove account** action that deletes the account's local mail (folders/messages/bodies) and its
keychain password, returning to the Add-account screen — so nothing of the account is left on the
device. And a verified guarantee that reading already-synced mail needs no network (OFF-1).

## In scope
- `geleit-engine::imap::delete_password(secrets, username)` (mirror of `store_password`).
- `geleit-app::refresh::run_remove_account(db_path, secrets)`: delete the account's keychain
  password, then `delete_account` (folders/messages/bodies cascade). Off the UI thread.
- UI: a **Remove account** control in the rail with an inline **confirm** (destructive →
  confirm-before-act); on confirm, wipe and return to the Add-account form.
- OFF-1: a deterministic test that synced mail reads back from the store with **no network**.

## Out of scope
- Encryption at rest (S2.2, deferred). Sync-integrity property tests (S2.7). Multi-account remove
  (single-account for now). Online/offline *detection*/indicator (later) — OFF-1 is the read path,
  which is already store-only (P1).

## Acceptance criteria (measurable)
1. build/test/`clippy -D warnings`/`fmt`/`cargo deny check` green.
2. `run_remove_account` wipes the account + its password (deterministic CI test: temp db +
   `InMemorySecretStore` — add account+settings+password → remove → account gone, password gone,
   folders/messages/bodies gone). No network.
3. OFF-1: a test reads back synced messages + body purely from the store (no network).
4. UI: Remove account requires a confirm; on confirm the app wipes and shows the Add-account form.
   Runs off the UI thread (P1); no secret/PII in errors (P2).
5. `cargo mutants` — store cascade covered; `refresh.rs`/`imap.rs` excluded; 0 missed.

## Deliverables
- `delete_password`; `run_remove_account` + CI test; Remove-account UI + wiring; OFF-1 test;
  `docs/manual/` "Removing an account". *(No new ADR.)*
