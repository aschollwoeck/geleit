# S4.2 — Message building · Plan (the HOW)

- Dep: `mail-builder` 0.4 (sibling of mail-parser; ADR-0009).
- `Draft` struct; `build()` uses `MessageBuilder::new().from(name?,addr).to(addrs).cc(addrs?)
  .subject().text_body().write_to_vec()`. mail-builder auto-fills Date + Message-ID.
- `recipients()` = To ++ Cc (bare) for `smtp::envelope`.
- Tests: well-formed (raw contains headers/body + Date/Message-ID), mail-parser round-trip,
  to-only/cc-only build, reject empty sender/recipients; e2e build→send→sink in `tests/smtp_send.rs`.

## Verify
gates; unit + e2e tests; mutants 0-missed; deny (mail-builder tree).
