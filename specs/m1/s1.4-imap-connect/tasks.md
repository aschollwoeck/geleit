# S1.4 — Connect to one IMAP account & list folders · Tasks (done vs. to-do)

Derived from `spec.md` + `plan.md` (P8). Real workspace code; full guidelines.
Status: `[ ]` todo · `[~]` in progress · `[x]` done.

## Build
- [x] Engine deps: tokio, async-imap (runtime-tokio), tokio-rustls, rustls (ring), futures,
      webpki-roots, rustls-pki-types, thiserror; engine depends on geleit-store
- [x] `geleit-store::upsert_folder` (INSERT OR IGNORE) + test
- [x] `geleit-engine::imap`: ImapConfig, ImapError, rustls config (dev verifier + webpki-roots),
      list_folders, persist_folders, sync_folders
- [x] `pub mod imap;` in engine lib

## Verify (acceptance criteria — measurable)
- [x] AC1 build/test/clippy -D warnings/fmt/`cargo deny check` green
- [x] AC2 LIVE (#[ignore], local): list_folders vs Dovecot returned INBOX (TLS login OK)
- [x] AC3 unit: missing password → NoPassword (no socket); persist_folders idempotent + scoped
- [x] AC4 `cargo mutants` (engine+store): 20 caught / 6 unviable / 0 missed (imap.rs excluded
      via .cargo/mutants.toml — live-tested, not unit-tested)
- [x] AC5 ADR-0006 recorded; workspace doc updated

## Document
- [x] `docs/adr/0006-imap-transport-stack.md`
- [x] `docs/technical/workspace.md`
- [x] (No end-user manual)

## Ship
- [x] Code review (guidelines §11, security-focused) — confirmed no credential/PII in errors/logs,
      correct stream/borrow/logout ordering, correct `upsert_folder`. Acted on findings:
      **gated the accept-any-cert path behind a `dangerous-tls` cargo feature** (release/CI builds
      can't bypass TLS → `InsecureTlsUnavailable`), corrected the overstated "still verifies
      signatures" comment (no MITM protection), made logout best-effort, and noted the
      spawn_blocking + zeroize follow-ups for the real keychain backend.
- [x] Update this tasks file to all-done
- [ ] PR merged (one-slice-one-PR, §12)