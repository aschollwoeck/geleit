# S1.7 — Minimal Slint shell · Tasks (done vs. to-do)

Derived from `spec.md` + `plan.md` (P8). UI slice.
Status: `[ ]` todo · `[~]` in progress · `[x]` done.

## Build
- [x] `geleit-app::viewmodel`: `MessageVm`, `message_vm`, `format_date` + tests
- [x] Slint UI (inline) to `design.md`: Palette global; 3 regions; virtualized message ListView
      (row design + guide edge); reading-pane placeholder; folder list + `folder-selected`
- [x] `main.rs`: open store, load folders + messages, build models, wire folder selection (no network)
- [x] `docs/manual/reading-mail.md` "seeing your mail" entry
- [x] `.cargo/mutants.toml`: exclude app `main.rs`; geleit-app added to CI mutants set

## Verify (acceptance criteria — measurable)
- [x] AC1 build/test/clippy -D warnings/fmt/`cargo deny check` green (+ Slint licensing: ADR-0007)
- [x] AC2 viewmodel mapping + date format unit-tested
- [x] AC3 app launches + renders against a real Dovecot-seeded store with no error (verified);
      visual fidelity per the `design.md` mockup. (Live desktop screenshot omitted — would capture
      the user's screen; `gnome-screenshot -w` grabs the focused window, not the app.)
- [x] AC4 UI path is store-only (no IMAP/network call in the shell)
- [x] AC5 `cargo mutants` geleit-app: 3 caught / 1 unviable / 0 missed (main.rs excluded)

## Ship
- [x] Code review (guidelines §11) — verdict sound: store-only path, `Rc<Store>` sound, no panics,
      licensing/lints correct. Acted on findings: dropped dead deps (engine/core, back in S1.9),
      folder hit-target 36→40px (§9), list font weights/sizes aligned to §3, out-of-range date
      test. TODO'd remaining polish (paperclip icon §7, Hanken Grotesk font §3, avatar/hover,
      selection guide edge in S1.8).
- [x] Update this tasks file to all-done
- [ ] PR merged (one-slice-one-PR, §12)