# S1.6 — Fetch + MIME-parse plaintext bodies · Tasks (done vs. to-do)

Derived from `spec.md` + `plan.md` (P8). Real workspace code; full guidelines.
Status: `[ ]` todo · `[~]` in progress · `[x]` done.

## Build
- [x] `geleit-engine::mime`: `ParsedBody`, `parse_body`, `make_snippet` + tests
- [x] `geleit-store`: `store_body` (atomic body + message update), `message_id_by_uid`, `body_for`
      (+ `StoredBody`) + tests
- [x] `geleit-engine::imap::sync_bodies` (fetch BODY.PEEK[], parse, store by uid)
- [x] `mod mime;` in engine lib

## Verify (acceptance criteria — measurable)
- [x] AC1 build/test/clippy -D warnings/fmt/`cargo deny check` green
- [x] AC2 LIVE: append multipart → sync envelopes+bodies → body/snippet/has_attachments stored ✓
- [x] AC3 unit: parse_body (multipart + plain-only + unparseable), make_snippet, store_body atomic,
      message_id_by_uid/body_for
- [x] AC4 `cargo mutants` (store + mime; imap.rs excluded): 63 caught / 0 missed
- [x] AC5 n/a

## Ship
- [x] Code review (guidelines §11, hostile-MIME aware) — confirmed no-panic parse, atomic
      store_body (FK-fail rolls back), no PII leak. Fixed two real bugs: **(HIGH)** envelope
      re-sync no longer wipes body-derived snippet/has_attachments (dropped from the upsert UPDATE
      set — the S1.5 TODO); **(MED)** `parse_body` now picks the genuine text/html part (not
      escaped leading plaintext). Added the §5 NOTE (MIME on executor), a body-already-present
      skip, `snippet` on `MessageHeader`, and tests (preserve-on-resync, FK-fail, real-html-part).
- [x] Update this tasks file to all-done
- [ ] PR merged (one-slice-one-PR, §12)