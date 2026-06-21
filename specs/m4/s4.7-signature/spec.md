# S4.7 — Per-account signature (SEND-7 / ACC-7) · Spec (the WHAT)

Slice of **M4 (Send)**. Type: store + engine + UI. A per-account signature, set when configuring the
account and auto-appended (below the standard `-- ` delimiter) to new messages, replies, and forwards.

Status: **draft.**

## In scope
- Store: migration #7 (`signature` column), `update_signature` (empty clears → NULL) + `signature`.
- Engine: `message::signature_block(sig)` — `"\n\n-- \n{sig}"`, empty when blank (pure, tested).
- App: a **Signature** field in the Add-account form (saved by `run_setup`, pre-filled on reconnect);
  compose / reply / forward append the signature to the body.

## Out of scope
- Rich-text/HTML signature; per-identity signatures (single account for now); editing the signature
  outside the setup/reconnect form (a settings screen is later).

## Acceptance criteria (measurable)
1. build/test/`clippy -D warnings`/`fmt`/`cargo deny check` green.
2. Store signature round-trips + empty clears to None — tested.
3. `signature_block` uses the `-- ` delimiter and is empty when blank — tested.
4. New/reply/forward bodies include the signature (maintainer eyeballs); `run_setup` persists it.
5. `cargo mutants` — store + `message` 0-missed.

## Deliverables
- Migration + store methods + test; `signature_block` + test; form field + run_setup + compose append.
