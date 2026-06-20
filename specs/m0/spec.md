# M0 — Foundations & feasibility · Spec (the WHAT)

The *what* for milestone M0. The *how* is `plan.md`; the *done-vs-todo* tracking is `tasks.md`
(constitution P8). Governed by `constitution.md`, `guidelines.md`; derived from `roadmap.md`.

Status: **draft.**

---

## Why M0 exists

M0 makes the project *buildable* and *de-risks the riskiest assumption in the whole plan*:
that a **native** (Slint) client can render real HTML email safely and scroll a large mailbox
smoothly. If either is infeasible, we want to know now — before building M1+ on that
assumption (constitution P4, P6). M0 ends with a committed, evidence-backed UI decision.

**M0 is infrastructure: it delivers no user stories** (`stories.md`). Its acceptance criteria
are therefore **measurable spike outcomes and a working scaffold**, not story-mapped acceptance
tests.

### Process adaptation for M0 (explicit, per P8 — not silent drift)
The full per-slice workflow (guidelines §11, constitution P10) assumes user-facing behavior.
M0 has none, so for M0 only:
- **No end-user manual** (nothing user-facing yet). Resumes at M1.
- **"Acceptance tests" → spike pass/fail criteria** measured and recorded (below). The scaffold
  is self-proving via green CI.
- **Technical documentation still applies** (scaffold layout, spike findings).
- **ADR(s) still apply** — the UI-framework decision is recorded as an ADR.
- **Code review still applies** to the scaffold and any code we intend to keep.
- **Spike code is throwaway** and need not meet full guidelines; it must not be carried into
  M1 without a rewrite that does.

---

## Goals

1. A compiling Cargo workspace with the engine/UI crate split and green CI.
2. Evidence on whether **safe native HTML email rendering** is feasible (spike).
3. Evidence on whether **Slint handles a large virtualized message list** (spike).
4. A committed UI-framework decision (Slint, or a documented pivot), recorded as an ADR.
5. Platform-abstraction seams in place so cross-platform isn't deferred risk.
6. OAuth app-registration paperwork started (long lead time).

## Deliverables & acceptance criteria (by slice)

### S0.1 — Workspace scaffold + CI
- Cargo **workspace** builds on Linux; `rust-toolchain.toml` pins stable.
- Crate seams exist and enforce the boundary: **engine crates contain no UI types and do not
  depend on the UI crate** (guidelines §2).
- **CI is green** and gates on: `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`,
  and `cargo mutants` configured (wired and runnable; thresholds tuned later).
- **Accept when:** a trivial change PR runs CI and all gates pass/fail correctly.

### S0.2 — Spike: safe HTML email rendering
- **Approach (decided): a sandboxed webview *component*** — a web engine embedded *only* as
  the isolated HTML message renderer, never as the app shell. This is the P4 "native" carve-out:
  the app stays Slint; the webview is contained to the message pane. (The plan picks the specific
  webview library and sandboxing mechanism.)
- Render a small **corpus of real-world HTML emails** (e.g. a newsletter, a receipt, a
  multipart message) in that sandboxed component.
- **Hard invariant — the privacy gate:** with remote-content blocking on, rendering a message
  produces **zero outbound network requests** (measured, e.g. via a network monitor/proxy).
- **No script execution.**
- Runs on Linux with a *credible, identified* path to macOS + Windows.
- **Accept when:** the corpus renders with acceptable fidelity, the zero-network invariant is
  demonstrated, and the approach (which engine/technique) is documented with its risks.

### S0.3 — Spike: virtualized message list (Slint)
- A Slint list rendering **~50,000 synthetic rows**, each with realistic content (sender,
  subject, snippet, date, unread, attachment indicator).
- **Smooth scroll: frame time within the 60fps budget (~16ms)** on the dev machine.
- **Bounded memory** — virtualization actually recycles rows (memory does not scale with row count).
- **Accept when:** 50k rows scroll at ≥60fps with bounded memory, measured and recorded.

### S0.4 — UI-framework decision (ADR)
- An ADR records the decision **Slint** (default if S0.2 + S0.3 pass) or a **pivot** (e.g. if
  safe native HTML rendering proves infeasible), with the evidence and the decision rule used.
- **Accept when:** the ADR is merged and the rest of the roadmap's UI assumption is confirmed
  or explicitly revised.

### S0.5 — Platform-abstraction seams
- Trait-level seams (interfaces, may be stubbed) for the OS-divergent components: **secret
  storage (keychain)**, **HTML render host**, **OAuth loopback** — with a Linux implementation
  path and named stubs for macOS/Windows. Linux is the primary dev OS.
- **Accept when:** the seams compile, the engine depends only on the traits, and the
  cross-platform strategy is documented.

### S0.6 — OAuth app-registration (parallel admin track)
- Google and Microsoft OAuth **app registrations initiated** (this is process, not code).
- **Accept when:** both applications are submitted/in progress and their status + expected
  lead time are recorded (so M7 isn't blocked by surprise).

## Out of scope for M0

- Any real mail sync, storage, search, sending, or organizing (those begin at M1).
- Any production UI feature — the Slint spike is a throwaway harness, not the app shell.
- Encryption-at-rest implementation (M2), OAuth *integration* (M7).
- Finalizing the UI conventions in guidelines §13 (done once the framework is committed, but
  the *commitment itself* is S0.4).

## Dependencies & constraints

- GitHub remote already exists; **CI = GitHub Actions** (decided).
- Primary dev OS: Linux. Provider auth: develop later milestones against local IMAP /
  Gmail app-password; Outlook needs OAuth (M7).

## Risks

- **HTML rendering is the existential risk** — no safe native Rust HTML engine exists, so the
  decided approach is a sandboxed webview *component* (S0.2). The spike must prove the
  zero-network privacy invariant and a cross-platform sandboxing path; failure forces a
  fallback (Servo/Verso) or a broader pivot. S0.2 is the highest-value slice; do it early.
- Slint's large-list performance is plausible but unproven for our row complexity (S0.3).
- OAuth approval lead time is outside our control (S0.6) — hence starting at M0.

## Open questions for the plan (`plan.md`)

1. **Which webview to embed and how to sandbox it cross-platform** — the S0.2 *approach* is
   decided (sandboxed webview component); the plan picks the specific library and the
   sandboxing/isolation mechanism (process isolation, no-network enforcement) per OS.
2. Exact crate layout and names for the engine/UI split.
3. CI config specifics and `cargo mutants` thresholds (provider = GitHub Actions, decided).
4. Source/composition of the real-world HTML email test corpus.
5. Slint version and the app-architecture pattern (state flow engine → UI).

## Exit criteria (M0 is done when)

Scaffold builds with green CI · both spikes executed with **recorded measurements** · the
UI-framework ADR is merged · platform seams compile · OAuth registrations are in progress —
and `tasks.md` reflects all of the above as done.
