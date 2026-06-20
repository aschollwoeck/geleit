# S1.5 — Sync a folder's recent envelopes · Tasks (done vs. to-do)

Derived from `spec.md` + `plan.md` (P8). Real workspace code; full guidelines.
Status: `[ ]` todo · `[~]` in progress · `[x]` done.

## Build
- [x] `geleit-store`: `NewMessage`, `upsert_message` (ON CONFLICT update), `MessageHeader`,
      `messages_in_folder` (newest-first) + tests
- [x] `geleit-engine::envelope`: pure `decode_header` + `address_parts` + tests
- [x] `geleit-engine::imap`: `connect` helper (refactored `list_folders`), `fetch_to_new_message`,
      `sync_envelopes`

## Verify (acceptance criteria — measurable)
- [x] AC1 build/test/clippy -D warnings/fmt/`cargo deny check` green
- [x] AC2 LIVE (#[ignore], --features dangerous-tls): append → sync_envelopes → subject in store ✓
      (bug found & fixed during verification: multi-item FETCH query needs parentheses)
- [x] AC3 unit: upsert insert+update (no dup); messages_in_folder newest-first+scoped; envelope helpers
- [x] AC4 `cargo mutants` (store + envelope module covered; imap.rs excluded): 38 caught / 0 missed
- [x] AC5 n/a (uses ADR-0006)

## Document
- [x] `docs/technical/workspace.md` (engine: envelope module + sync_envelopes)
- [x] (No end-user manual)

## Ship
- [x] Code review (guidelines §11) — confirmed SQL upsert, window math, no-panic mapping, borrow
      ordering, no PII leak. Acted on findings: **extracted `recent_window` into `envelope.rs`**
      (the off-by-one math was the only untested logic → now unit + mutation tested); **skip
      NULL-UID messages** (would duplicate on re-sync, P6); added notes for the async/!Send DB
      writes and for S1.6 not to clobber has_attachments/snippet.
- [x] Update this tasks file to all-done
- [ ] PR merged (one-slice-one-PR, §12)