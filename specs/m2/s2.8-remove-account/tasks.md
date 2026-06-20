# S2.8 — Remove account (wipe) + offline reading · Tasks

Derived from `spec.md` + `plan.md` (P8). App + engine-glue slice.
Status: `[ ]` todo · `[~]` in progress · `[x]` done.

## Build
- [x] engine `imap::delete_password`
- [x] app `refresh::run_remove_account` (→ `Ok(password_cleared)`) + CI test (temp db + InMemory)
- [x] UI: Remove-account control + inline confirm (`confirm-remove`), `remove-account()` callback
- [x] `main.rs` wiring: worker → run_remove_account → reload (→ Add-account form); warn if pw lingered
- [x] OFF-1 test (store-only read of synced mail, no network)
- [x] `docs/manual/` "Removing an account" + "Reading offline"

## Verify (acceptance criteria)
- [x] AC1 build/test/clippy/fmt/deny green
- [x] AC2 run_remove_account wipes account + password + mail (incl. body cascade) — deterministic,
      no network/keychain (temp db + InMemorySecretStore); idempotent
- [x] AC3 OFF-1 offline read test (store-only)
- [x] AC4 UI confirm-before-wipe (private `confirm-remove`); off the UI thread; no PII in errors;
      Remove button danger-strong-on-surface (AA ~5.45:1)
- [x] AC5 mutants store+engine: 89 caught / 8 unviable / 0 missed (store cascade covered;
      refresh.rs/imap.rs/os_secret.rs excluded)

## Ship
- [x] Code review (guidelines §11) — verdict sound (wipe complete + ordered, idempotent, confirm
      gates the destructive call, threading/P2 clean). Fixed the one finding: a keychain-delete
      failure is no longer hidden — `run_remove_account` returns whether the password was cleared
      and the UI warns on the form if it wasn't. Added a body row to the wipe test.
- [x] Update this tasks file to all-done
- [ ] PR merged (one-slice-one-PR, §12)