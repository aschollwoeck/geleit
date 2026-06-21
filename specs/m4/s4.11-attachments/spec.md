# S4.11 — Attachments in compose (SEND-4) · Spec (the WHAT)

Slice of **M4 (Send)**. Type: engine + UI. Attach files to an outgoing message; they ride along as
MIME parts and arrive as downloadable attachments.

Status: **draft.**

## In scope
- Engine: `Draft.attachments: Vec<Attachment{filename, content_type, data}>`; `build()` adds each via
  mail-builder; `guess_content_type(filename)` (by extension). Pure, tested incl. a parse-back.
- App: in compose, a path field + **Attach** button reads the file into memory and lists it
  (name · size) with **Remove**; attachments ride through `run_send`.

## Out of scope
- **Native file picker** (rfd/portal) — a follow-up; it spins its own GTK modal loop and can't be
  safely added/tested without a display. For now: type/paste a path.
- Persisting attachments in saved drafts (drafts keep text only — noted).

## Acceptance criteria (measurable)
1. build/test/`clippy -D warnings` (incl. `--features dangerous-tls`)/`fmt`/`cargo deny check` green.
2. A built message with an attachment parses back with the right filename + bytes; `guess_content_type`
   maps known extensions + falls back — tested (every arm).
3. App: Attach reads a path → lists it; Remove drops it; a sent message carries the attachment; cleared
   on new/reply/forward/resume (maintainer eyeballs).
4. `cargo mutants` — `message` 0-missed.

## Deliverables
- Engine attachments + `guess_content_type` + tests; compose attach-by-path UI + state; `run_send`
  carries attachments.
