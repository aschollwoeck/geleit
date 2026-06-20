# S1.9 — Manual refresh · Tasks (done vs. to-do)

Derived from `spec.md` + `plan.md` (P8). UI + integration slice (last of M1).
Status: `[ ]` todo · `[~]` in progress · `[x]` done.

## Build
- [x] `geleit-engine::imap::store_password` helper
- [x] `geleit-app::refresh`: `build_imap_config` (+ tests), `config_from_env`, `run_refresh`
- [x] app deps: geleit-engine, geleit-platform, tokio (rt/net/time); `dangerous-tls` feature → engine
- [x] Slint: Refresh button + status banner; `refreshing`/`status` props; `danger-*` tokens; `refresh()`
- [x] `main.rs` wiring: off-thread sync + `invoke_from_event_loop` reload (nothing `!Send` crosses)
- [x] `.cargo/mutants.toml`: exclude `geleit-app/src/refresh.rs`
- [x] `docs/manual/reading-mail.md` updated (refresh)

## Verify (acceptance criteria — measurable)
- [x] AC1 build/test/clippy -D warnings/fmt/`cargo deny check` green
- [x] AC2 `build_imap_config` validates host/user/port (5 tests)
- [x] AC3 LIVE (`--features dangerous-tls`): `run_refresh` syncs INBOX from Dovecot (test passes);
      app launches with the button/banner; UI responsive (off-thread). Literal button-click is manual.
- [x] AC4 P1: sync runs on a worker thread + `invoke_from_event_loop`; only `Send` data crosses
- [x] AC5 `cargo mutants` store+app: 34 caught / 8 unviable / 0 missed (refresh.rs+main.rs excluded)

## Ship
- [x] Code review (guidelines §11) — verdict: threading core sound, P1/P2 hold (nothing `!Send`
      crosses, errors discarded so no PII in banner). Fixed: M1 button AA (accent-strong now on
      surface), M2 danger tokens to design.md (#b3472e/#fbe9e4), L3 `catch_unwind` (button never
      stuck), L4 refresh now syncs+reloads the selected folder, L5/L6 honest comments. Folder-list
      live-update remains out of scope (next launch).
- [x] Update this tasks file to all-done
- [ ] PR merged (one-slice-one-PR, §12) — **completes M1**