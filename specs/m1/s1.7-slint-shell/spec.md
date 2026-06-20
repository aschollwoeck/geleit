# S1.7 — Minimal Slint shell · Spec (the WHAT)

Slice of **M1** (`roadmap.md`). Type: UI. First visible UI. Delivers **READ-1** (message list,
newest-first), **READ-2** (row shows sender/subject/snippet/date/unread/attachment), and folder
navigation (READ-6, list side). Built to `design.md`; reads the **local store only** (P1 — no
network in the UI). End-user manual applies (first user-facing slice) — but the read view is
partial (no body until S1.8), so the manual entry is minimal and grows with S1.8.

Status: **draft.**

## Purpose
Stand up the Slint app shell — the "Soft daylight" three-region layout (folders · message list ·
reading pane) — showing folders and a **virtualized** message list from the local store, so a
person can see their synced mail. The reading pane is a placeholder until S1.8.

## In scope
- `geleit-app` becomes a Slint app built to `design.md` tokens (palette, type, spacing, the guide
  edge, the calm density).
- A pure `viewmodel` module (mutation-tested): map `geleit-store` rows → display values
  (sender/subject/snippet fallbacks, date formatting), independent of Slint.
- Open the store, load the first account's folders + the selected folder's messages (newest-first,
  virtualized `ListView`), render. Folder click reloads the list. **No network.**
- A minimal `docs/manual/` entry for "seeing your mail" (grows in S1.8).

## Out of scope
- Reading-pane body content (S1.8); refresh/sync wiring (S1.9 — the store is read as-is here);
  selection→open, compose, organize, search; account setup UI (M7).

## Acceptance criteria (measurable)
1. `cargo build/test --workspace` + `clippy -D warnings` + `fmt` + `cargo deny check` green.
2. `viewmodel` maps store rows → display rows correctly (fallbacks + date format), unit-tested.
3. The app **runs and renders** the Soft-daylight shell from a populated store — folders in the
   rail, a virtualized message list (sender/subject/snippet/date, unread dot, attachment marker).
   Captured as a screenshot for review.
4. The UI path touches the store only (no IMAP/network call in the shell).
5. `cargo mutants` covers the `viewmodel` (UI/`main` excluded) — 0 missed.

## Deliverables
- `geleit-app` Slint UI + `src/viewmodel.rs`; `docs/manual/` entry. *(Screenshot in PR.)*
