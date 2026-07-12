# GeleitMail — Style & Implementation Guidelines

The single written standard referenced by `constitution.md` P9. New code matches the
surrounding code and these rules. Where a rule and the constitution conflict, the
constitution wins. The UI framework was committed to **Tauri + Leptos** in M9 (ADR-0012, superseding ADR-0001); the UI
conventions in §13 are final.

Status: **initial draft.** Refine as the codebase grows; amend deliberately, not silently.

---

## 1. Language & toolchain

- **Rust, latest stable**, 2021 edition (revisit 2024 when ready). Pin via `rust-toolchain.toml`.
- `cargo fmt` is mandatory; default `rustfmt` settings, no per-file overrides. CI fails on diff.
- `cargo clippy` clean with `-D warnings`. Don't `#[allow(...)]` to silence — fix it, or
  justify the allow inline with a comment.
- No warnings in committed code.

## 2. Workspace & module boundaries

- **Cargo workspace**, multiple crates. The **engine is UI-agnostic** (constitution P4/P6):
  no UI type ever appears in engine crates, and the engine never depends on a UI crate.
- Rough crate seams (names provisional): `store` (SQLite + encryption), `sync` (IMAP),
  `mime`, `search` (tantivy), `transport` (SMTP), `auth` (OAuth), a `core`/domain crate for
  shared types, and a separate UI crate. A UI-framework swap must touch only the UI crate.
- Public API of each crate is deliberate and documented; keep internals private. Prefer
  small, well-named modules over large files.

## 3. Naming

- Standard Rust conventions: `snake_case` items, `CamelCase` types, `SCREAMING_SNAKE` consts.
- Names describe intent in the domain (Account, Mailbox, Message, Envelope, Thread, SyncState),
  not the mechanism. Avoid abbreviations except well-known ones (IMAP, MIME, UID).
- No Hungarian notation, no type-name suffixes like `MessageStruct`.

## 4. Error handling

- **Never `unwrap()`/`expect()`/`panic!` on recoverable paths**, and never on anything
  touching the user's mail, the network, or stored data. Panics are for genuine invariant
  violations only, with a message that explains the invariant.
- **Library/engine crates:** typed errors via `thiserror`; return `Result<T, ThisCrateError>`.
  Don't leak third-party error types across crate boundaries — wrap them.
- **Binary/app layer:** `anyhow` (or equivalent) for top-level composition and context.
- Add context as errors propagate (`.context("syncing folder INBOX")`) — errors must be
  diagnosable without a debugger.
- **No error message, log, or panic ever contains mail content, addresses, tokens, or other
  PII** (constitution P2). This is a hard rule, checked in review.

## 5. Async & concurrency

- **Every interaction must feel instantaneous.** This is a product value, not a nice-to-have: a
  native local-first client justifies itself by feeling immediate. Treat perceptible latency on a
  local action (folder switch, opening a mail, rendering content) as a **defect** — fix the cause
  (right data structures, no redundant work or blocking calls in a click handler; build expensive
  things like the webview once, up front, not lazily on first use). Only where work genuinely takes
  real time (network sync) is **immediate visual feedback** the answer — and it's the fallback, not
  the goal. Always judge performance on a `--release` build; debug is 10–50× slower and misleading.
- `tokio` as the async runtime. All IMAP/SMTP/network I/O is async.
- **The UI never blocks on the network (P1).** Engine work runs off the UI thread; the UI
  observes the local store and receives updates. CPU-heavy work (MIME, indexing) uses
  `spawn_blocking` or a dedicated pool — never on an async executor thread, never on the UI.
- Sync operations are **cancellable** and **resumable** (P6): design for interruption at any
  point with no corruption.
- Prefer message-passing/channels over shared mutable state. If a lock is held across an
  `.await`, justify it; never hold a lock across network I/O.

## 6. Dependencies

- **Minimal and justified.** Every new dependency is a decision: prefer std and a few
  well-maintained crates over many small ones. Note non-obvious additions in the milestone plan.
- **No dependency that phones home, collects telemetry, or requires a network service we run**
  (P2). Audit transitive deps for this.
- Run `cargo deny`/`cargo audit` in CI for advisories and license compatibility (MIT-compatible).
- Pin the committed lockfile.

## 7. Testing

- **Unit tests** live with the code; **integration tests** in `tests/`. Tests are
  deterministic and **never hit the live network** — IMAP/SMTP/OAuth are tested against
  fakes/recorded fixtures.
- **Mutation testing (`cargo mutants`) on the core** — store, sync, MIME, crypto (P6). A high
  surviving-mutant rate in these crates is a defect, not just low coverage.
- Every bug fix gets a regression test. Sync/MIME correctness gets property-based tests where
  it fits (e.g. `proptest`) — round-trips, idempotency, no-loss invariants.
- Tests must be fast enough to run on every change; gate slow/networked suites separately.

## 8. Documentation

- Public items have `///` doc comments explaining *why/what*, not restating the signature.
- Non-obvious decisions are recorded as short **ADRs** (e.g. `docs/adr/NNN-title.md`),
  especially the M0 UI-framework decision and the encryption/key-management choice.
- Module-level `//!` docs state each module's responsibility and boundary.
- Comments match the surrounding density; explain intent, not mechanics.

## 9. Security-critical code (P6)

- HTML rendering, crypto, OAuth token handling, and MIME parsing get extra scrutiny: the
  smallest surface, the clearest invariants, the strongest tests, and a security review
  before release.
- Secrets (tokens, keys) live only in the OS keychain, never in the SQLite store in plaintext,
  never in logs, never in source/config. Zeroize in-memory secrets where practical.
- Remote content and scripts in mail are blocked by default and treated as hostile input.

## 10. Logging & observability

- Logging is **local-only and opt-in to verbosity** (`tracing`). Default level reveals no PII.
- No log shipping, no crash-reporting service that transmits off-device, unless explicitly
  user-initiated and consented (P2).

## 11. Slice implementation workflow (the build step)

Once a slice's **spec, plan, and tasks** are agreed (P8), it is built in this exact order
(this is the "build" step of the chain; see constitution P10 for the commitment):

1. **Build** — implement the slice's source code per its spec and plan, on its own branch (§12).
2. **Acceptance tests** — write test cases that verify the slice. These **double as the
   executable acceptance criteria for the user stories** the slice serves: every affected
   user story (from the slice's spec) maps to one or more tests, named/tagged so the mapping
   is obvious (e.g. `acceptance_<story-id>_...`). Cover the happy path, failure modes, and the
   integrity invariants (no mail loss/duplication) where relevant. *A slice with no user story
   (infrastructure or a throwaway spike) instead records measurable pass/fail criteria.*
3. **Run** — everything green: `cargo test` + `cargo fmt --check` + `clippy -D warnings`
   **run for the host *and* the `wasm32-unknown-unknown` target** (a wasm-only break in the Leptos
   frontend passes a host-only check, §13) + `cargo mutants` on the touched pure logic (§8, §13) +
   `cargo deny check` + `scripts/check-boundary.sh`. Red means not done.
   - **Verify by the means the slice needs, beyond the automated suite.** **UI slices are
     screenshot-verified:** launch the app against a **seeded demo DB** — *never a real account*, and
     target your own window by PID so you don't capture a running user instance — and check each
     affected view in light and dark. **Engine / network slices are live-verified** against a local
     IMAP/SMTP (Dovecot) with the `#[ignore]`d live tests. Pure logic is covered by the tests above.
4. **User manual** — write the slice's user-facing behavior into the **end-user manual**
   (`docs/manual/`) and keep it current. Plain language for private people, not engineers — what
   they can now do and how. The manual is a **maintained, required artifact**, written per slice and
   never deferred to a "docs later" phase. *A slice with no user-facing behavior (infrastructure or a
   throwaway spike) omits this step — note the omission in the PR.*
5. **Technical manual** — write the slice into the **technical manual** (`docs/technical/`) and keep
   it current: architecture, data flow, the decisions, how the slice works and why. Also a
   **maintained, required artifact**. This is *in addition to* in-code `///` docs and ADRs (§8), not
   a replacement.
6. **Review — a panel, not one pass.** Each reviewer below is a **separate agent** with its own,
   single remit; **spawn them in parallel** over the slice's diff so the passes are independent (one
   lens never colours another) and the review is fast. **Every reviewer checks its artifact is present
   *and* of good quality**, not merely present, and **every warranted finding is addressed before
   merge**:
   - **Code reviewer** — correctness, the constitution, and these guidelines; failure modes and the
     integrity invariants (no mail loss/duplication).
   - **Tests reviewer** — the acceptance tests (step 2) were actually written, map to the slice's
     user stories, cover the happy path / failure modes / integrity invariants, and are meaningful
     (not vacuous, tautological, or asserting nothing).
   - **Technical-manual reviewer** — the technical manual (step 5) was updated for this slice and is
     accurate, complete, and clear.
   - **User-manual reviewer** — the user manual (step 4) was updated and is correct, in plain
     language, and covers what the user can now do.
7. **Merge** — open and merge the PR per §12 once all of the above pass.

The user and technical manuals (steps 4–5) are produced per slice and **accumulate incrementally** —
they become extensive over the project's life, and are never deferred to a "docs later" phase; each
is checked by its own reviewer in step 6.

## 12. Git & commits

- **One slice = one branch = one PR.** Each slice is developed on its own branch and **merged
  into `main` at the end via a pull request** — never committed straight to `main`. The PR is
  the slice's completion gate.
- A slice's PR is mergeable only when the full §11 workflow has passed: end-to-end and
  verifiable (P8), **all gates green** (§11 step 3), the **user manual and technical manual**
  written/updated (where the slice warrants it), **every reviewer in the §11 panel addressed**
  (code · tests · technical-manual · user-manual), and the slice's `tasks.md` updated to mark it done.
- The PR description states *what* the slice delivers and links the milestone spec/plan; the
  diff is reviewed against the constitution and `guidelines.md` before merge.
- **CI mirrors the §11 gates on every PR** (fmt · clippy `-D warnings` · tests · `cargo deny` ·
  mutants-on-diff · boundary — ~2 min). The slow **release perf-budget** (fat-LTO build + cold-start /
  RSS measurement) runs on **merge to `main`**, not per-PR, so PR feedback stays fast while it still
  gates what lands; the pre-tag release build is the hard perf backstop.
- Small, focused commits within the branch; imperative subject lines that say *why*.
- `main` stays buildable and test-green at all times.
- Don't commit secrets, fixtures containing real mail, or generated artifacts.

## 13. UI conventions (Tauri + Leptos)

The shell is **Tauri** (OS webview) and the UI is **Leptos** (Rust → WASM), committed in M9 after
both native-HTML-rendering routes failed ([ADR-0012], superseding ADR-0001/0011). No npm — `cargo`
and `deny.toml` cover the whole tree. Details: `docs/technical/tauri-shell.md`.

- **Two crates, one boundary.** `geleit-app` is the Tauri host (window, IPC commands, the `mail://`
  origin); `geleit-ui` is the Leptos frontend. **`geleit-ui` depends on none of our crates** — it
  reaches the engine *only* over the typed IPC seam. `check-boundary.sh` enforces this: view code
  cannot touch the store even by accident.
- **No business logic in the UI.** Pure display logic (dates, elision, virtualization math) lives in
  `geleit-ui/src/view.rs`; store→UI mapping and pure resolvers live in `geleit-app/src/dto.rs`. Both
  are **mutation-tested**. Glue (IPC commands, view wiring) is excluded from mutants.
- **Never block the webview (P1).** Every IPC command is `async` and hops to a blocking thread; the
  store is opened once and kept behind a `Mutex`. Network work runs on a worker and streams results
  back (Tauri events for progress).
- **Large lists** are **virtualized** — render only the visible window (`view::visible_range`), never
  the whole list; reading `.with(Vec::len)` and cloning only the window keeps the scroll path O(1).
- **HTML email renders only in a sandboxed `<iframe>` on its own `mail://` origin** — never `srcdoc`
  (which inherits the app CSP and strips mail styles). The message body **never enters the app
  document**, not even as a string. Three layers: ammonia sanitizer, iframe `sandbox` with no
  `allow-scripts`/`allow-same-origin`, and `webview_document`'s `default-src 'none'` CSP. Remote
  content is blocked by default (PRIV-1); "Load images" is a per-message CSP relaxation, not a fetch.
- **No inline scripts.** `dist/*.js` are external files, so the app CSP stays a strict
  `script-src 'self'` (Tauri's nonce injection doesn't reach inline module scripts anyway).
- **Skeleton paint.** WebKit takes ~630 ms to boot; `index.html` paints the chrome statically first,
  so the window is never blank.
- **Calm/fast (P3).** Instant feedback from local state; long operations show non-blocking progress.
- **Accessibility & keyboard navigation** are first-class. **Theming:** light/dark from `design.md`'s
  token table (CSS custom properties), the choice persisted in the store.

[ADR-0012]: docs/adr/0012-tauri-shell-with-leptos-ui.md
