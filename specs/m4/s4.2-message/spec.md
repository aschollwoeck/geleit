# S4.2 — Message building (MIME) · Spec (the WHAT)

Slice of **M4 (Send)**. Type: engine. The **verifiable half** of roadmap S4.2: turn a composed
`Draft` into RFC 5322 wire bytes (`mail-builder`, ADR-0009) that `smtp::send` delivers (SEND-1
foundation). The **compose window** (UI, needs the maintainer's eyes) is the companion follow-on.

Status: **draft.**

## In scope
- `geleit-engine::message`: `Draft { from_name, from_addr, to, cc, subject, body_text }`,
  `build(&Draft) -> Result<Vec<u8>, String>` (auto Date + Message-ID via mail-builder), and
  `recipients(&Draft)` (To+Cc bare addresses for the envelope).
- Validation: a sender + at least one recipient, else a calm error.
- An **end-to-end test** (build → `smtp::send` → in-process sink) proving a drafted message is
  delivered with the right recipients + subject + body — runs in CI.

## Out of scope
- Compose window UI (next). Reply/forward headers (S4.3). Attachments (S4.4). HTML body / formatting
  (S4.6). Signature (S4.7).

## Acceptance criteria (measurable)
1. build/test/`clippy -D warnings`/`fmt`/`cargo deny check` green.
2. `build` produces a well-formed message (From/To/Cc/Subject/body + auto Date/Message-ID),
   round-trips through `mail-parser`, and rejects missing sender/recipients — tested.
3. End-to-end build→send→sink delivers To+Cc as envelope recipients + correct subject/body (CI test).
4. `cargo mutants` — `message` covered; 0 missed.

## Deliverables
- `engine::message` + unit tests + the e2e test; mail-builder dep.
