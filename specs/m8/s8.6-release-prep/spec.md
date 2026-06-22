# S8.6 — First-run polish + release prep · Spec

## In scope
- Version bump to **0.1.0** (workspace.package.version; crates use version.workspace; dropped the
  stale `version = "0.0.0"` pins on geleit-app's path deps).
- Empty states (APP-2 polish): the message list shows "No messages here yet." / "No messages match
  your search." / "Loading…" instead of a blank pane.
- `CHANGELOG.md` for v0.1.0; roadmap marks M8 done (Linux).

## Out of scope (maintainer's call)
- Actually tagging `v0.1.0` (pushing the tag triggers the public release build) and running a
  real-account beta — outward-facing release decisions left to the maintainer.

## Acceptance criteria
1. Workspace builds at 0.1.0; all gates green.
2. Empty states render (maintainer eyeballs); CHANGELOG + roadmap updated.

## Deliverables
- version 0.1.0; message-list empty state; CHANGELOG.md; roadmap M8 marked done.
