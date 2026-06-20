# S1.8 — Reading pane · Tasks (done vs. to-do)

Derived from `spec.md` + `plan.md` (P8). UI slice.
Status: `[ ]` todo · `[~]` in progress · `[x]` done.

## Build
- [x] `geleit-store::set_seen(message_id, bool)` + test
- [x] `geleit-app::viewmodel::body_display` + tests
- [x] Slint: `MessageItem.id`; selection (`message-selected(MessageItem)`, selected styling +
      guide edge); reading-pane content + ScrollView; `mark-unread`; wiring (load body, mark read,
      in-place row update)
- [x] `docs/manual/reading-mail.md` updated (opening a message)

## Verify (acceptance criteria — measurable)
- [x] AC1 build/test/clippy -D warnings/fmt/`cargo deny check` green
- [x] AC2 set_seen flips state; body_display four cases (tested)
- [x] AC3 app launches clean; reading-pane logic via tests; selection/guide edge per design.md
- [x] AC4 store-only UI path (no network)
- [x] AC5 `cargo mutants` geleit-store+geleit-app: 34 caught / 8 unviable / 0 missed (main.rs excl.)

## Ship
- [x] Code review (guidelines §11) — verdict sound (current-folder/mark-read wiring correct, P1/P2
      hold, id-0 sentinel safe). Fixed M1 (in-place `set_row_data` instead of full model rebuild —
      keeps scroll, no 1000-row re-query) and L2 (distinguish store read error from "not
      downloaded"). i32 id cast (L1) noted as latent-only; full-width unread TouchArea nit left.
- [x] Update this tasks file to all-done
- [ ] PR merged (one-slice-one-PR, §12)