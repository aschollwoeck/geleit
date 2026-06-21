# S4.10 — Drafts: save & resume · Tasks

## Build
- [x] store: migration #9 (draft table) + DraftContent/DraftRow + save/list/get/delete + round-trip test
- [x] app: Save-draft button + Drafts rail button + drafts list overlay + resume wiring
- [x] state: current_draft_id; run_send deletes the sent draft

## Verify
- [x] AC1 build/test/clippy -D warnings (+ dangerous-tls)/fmt/deny green
- [x] AC2 draft save→list→resume→update→delete round-trip incl. references (tested)
- [~] AC3 Save/Drafts/resume + delete-on-send — MAINTAINER eyeballs
- [x] AC4 mutants store 0-missed

## Ship
- [ ] tasks all-done; PR merged
