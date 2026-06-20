# GeleitMail — Style & Implementation Guidelines

The single written standard referenced by `constitution.md` P9. New code matches the
surrounding code and these rules. Where a rule and the constitution conflict, the
constitution wins. **UI-framework-specific sections are provisional until the framework is
committed in M0** and are marked *(provisional — pending M0)*.

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
3. **Run** — run the suite; everything green: tests + `cargo fmt --check` + `clippy -D warnings`
   + `cargo mutants` on touched core crates. Red means not done.
4. **End-user manual** — update the extensive end-user manual in `docs/manual/` for any
   user-facing behavior the slice adds or changes. Plain language for private people, not
   engineers. *A slice with no user-facing behavior (infrastructure or a throwaway spike)
   omits this step — note the omission in the PR.*
5. **Technical documentation** — update the extensive technical documentation in
   `docs/technical/`: architecture, data flow, decisions, how the slice works and why. This is
   *in addition to* in-code `///` docs and ADRs (§8), not a replacement.
6. **Code review** — run a code reviewer over the slice's diff (the `code-review` process),
   checking correctness, the constitution, and these guidelines; address findings.
7. **Merge** — open and merge the PR per §12 once all of the above pass.

Documentation (4, 5) is produced per slice and **accumulates incrementally** — it becomes
extensive over the project's life, and is never deferred to a "docs later" phase.

## 12. Git & commits

- **One slice = one branch = one PR.** Each slice is developed on its own branch and **merged
  into `main` at the end via a pull request** — never committed straight to `main`. The PR is
  the slice's completion gate.
- A slice's PR is mergeable only when the full §11 workflow has passed: end-to-end and
  verifiable (P8), test-green (fmt + clippy `-D warnings` + tests + `cargo mutants` on touched
  core crates), **manual (where the slice is user-facing) and technical docs updated**,
  **code review addressed**, and the slice's `tasks.md` updated to mark the slice done.
- The PR description states *what* the slice delivers and links the milestone spec/plan; the
  diff is reviewed against the constitution and `guidelines.md` before merge.
- Small, focused commits within the branch; imperative subject lines that say *why*.
- `main` stays buildable and test-green at all times.
- Don't commit secrets, fixtures containing real mail, or generated artifacts.

## 13. UI conventions *(provisional — pending M0)*

- Assumed Slint pending the M0 decision. The UI crate holds **no business logic** — it renders
  state from the engine and forwards intents back.
- Calm/fast (P3): every interaction targets instant feedback from local state; long operations
  show non-blocking progress, never a frozen UI.
- Accessibility and keyboard navigation are first-class, not afterthoughts.
- Finalize this section (widget patterns, state flow, theming) once the framework is committed.
