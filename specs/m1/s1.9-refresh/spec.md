# S1.9 — Manual refresh · Spec (the WHAT)

Slice of **M1** (last slice). Type: UI + integration. Delivers **SYNC-1** (the user can fetch new
mail on demand). Wires the engine's IMAP sync into the UI — **off the UI thread** (P1: the UI never
waits on the network), with calm feedback (P3, `design.md` §10).

Status: **draft.**

## Purpose
A **Refresh** control that syncs the current account from the server (folders + the inbox's
envelopes + bodies) **without freezing the UI**, then updates the message list — and tells the
person honestly when it's working and when it failed.

## In scope
- Slint: a Refresh button (shows "Refreshing…", disabled while busy) + an honest status/error
  banner (`design.md` §10).
- Off-thread sync: the refresh runs on a worker thread (its own tokio runtime + store connection);
  on completion it posts back to the UI thread (`invoke_from_event_loop`) to reload the list. The
  UI thread never blocks on the network (P1).
- `geleit-app::refresh`: `build_imap_config` (pure, tested) + `run_refresh` (network, off-thread).
- `geleit-engine::imap::store_password` helper so the app can supply the password to the seam.
- IMAP connection settings + password come from **environment** for now (the dev bridge — real
  account-setup UI is M7; real keychain is later). A `dangerous-tls` app feature forwards to the
  engine for self-signed dev servers.

## Out of scope
- Account-setup UI / real OS keychain (M7). Background/automatic sync + change detection (M2).
  Live-updating the **folder** list after refresh (new folders appear next launch). Writing local
  read-state back to the server (M6).

## Acceptance criteria (measurable)
1. build/test/`clippy -D warnings`/`fmt`/`cargo deny check` green.
2. `build_imap_config` validates host/user/port (tested).
3. **Live (`--features dangerous-tls`, env-configured):** append a new message to Dovecot → click
   Refresh → the message appears in the list; the UI stays responsive; failure shows the banner.
4. P1: the sync runs off the UI thread (worker thread + `invoke_from_event_loop`); no network call
   on the UI thread.
5. `cargo mutants` — `build_imap_config` tested; `refresh.rs` (network/threading) excluded like
   `imap.rs`; existing coverage unchanged, 0 missed.

## Deliverables
- `geleit-app::refresh` + UI refresh control/banner; `engine::imap::store_password`;
  `docs/manual/reading-mail.md` updated. *(No new ADR.)*
