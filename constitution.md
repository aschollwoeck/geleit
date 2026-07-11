# GeleitMail Constitution

The non-negotiable principles. Every spec, plan, and code change is checked against this
document. If a decision conflicts with the constitution, the constitution wins — or the
constitution is amended deliberately, never silently. This is what is *always* a priority.

---

## What we are building

A **native, local-first email client for private people** — not power users. You add your
accounts in a click, switch between them, and read/reply/organize your mail calmly and
instantly. It blocks trackers, sends nothing home, and keeps your mail encrypted on your
own machine. The *feel* — fast, quiet, private — is the product.

We are competing with Outlook, eM Client, and Mailbird, and with webmail. We win on
**integrity, native performance, and honest privacy** — not on feature count.

---

## Principles (in priority order)

### P1. Local-first — the UI never waits on the network
The local store is the source of truth for the *experience*. Every read, scroll, search,
and organize action operates on local data and returns instantly. Sync runs in the
background; its results simply appear. Blocking the UI on a network call is a bug, not a
slow path.

### P2. Privacy is the product — and we are honest about it
- **No telemetry.** Nothing about how the user uses the app ever leaves the device.
- **No middleman.** The app talks *directly* to the user's provider. We run no server that
  the user's mail or metadata passes through.
- **No tracking.** Remote content is blocked by default; opening a message never phones home.
- **Honest claims only.** Email inherently leaves the device — the provider has it. We never
  claim "your mail never leaves your device." We claim: *no middleman, no telemetry, no
  tracking.* Marketing that overstates this is forbidden; user trust is the whole product.

### P3. Calm and fast *is* the feature
Interactions are instant. The interface is quiet, uncluttered, and low-effort to
understand. Absence of noise (no ads, no clutter, no nagging) is a feature we protect.
A performance or clutter regression is treated as a defect.

### P4. Lean and Rust, not Electron
GeleitMail is a Rust application end to end — engine, store, and UI. It ships **no bundled
browser**: it uses the operating system's webview, so the binary stays lean (~10–20 MB, not
Electron's ~150 MB) and there is no second runtime to ship, patch, or trust.

Email is fundamentally a web document. Two attempts at native HTML rendering failed on their
merits — an embedded webview *component* proved unstable on X11 (ADR-0001), and a pure-Rust CPU
renderer could not render real mail correctly (ADR-0011). We accept the consequence: rendering
hostile HTML **correctly and safely** requires a real, hardened browser engine (ADR-0012).

So the shell is the system webview — and every message is confined **inside** it to a
script-free, CSP-locked sandbox that cannot execute code, reach the IPC bridge, or fetch
anything remote unless the reader asks.

*This principle was amended in M9. The original — "Native, not a webview shell" — rested on the
premise that a contained native/webview split was achievable. It was not; see ADR-0012.*

**Leanness is measured, not asserted.** CI fails the build if any ceiling is breached
(`scripts/perf-budget.sh`, run headless under xvfb — S9.8):

| Budget | Ceiling | Enforcement |
|---|---|---|
| Binary size (stripped) | **30 MB** | CI, every PR |
| Cold start (release, warm cache, to first paint) | **1200 ms** | CI, every PR (timed exec→first paint) |
| Idle RSS (window open) | **280 MB** | CI, every PR |
| Message-open latency (click → rendered) | **100 ms** | Architecturally bounded: the open path is a local SQLite read + a local iframe render — **no network** — so it cannot exceed this in practice. Not separately timed (no precise headless render-complete signal); revisit if the open path ever gains I/O. |

These are **ceilings, not targets**. A change that moves any of them toward its ceiling is a
defect to be justified, not a cost to be absorbed. Tightening a ceiling is always in scope;
raising one requires a written justification in the PR and an ADR if it is structural.

### P5. Built for private people, not power users
Default to simplicity and safe defaults over configurability. Setup is effortless. No
master-password friction — encryption at rest is transparent (OS keychain). When a choice
trades power-user flexibility for regular-person clarity, choose clarity.

### P6. Integrity of the user's mail is sacred
Never lose, duplicate, or corrupt mail. Sync correctness comes before features. Mail is
encrypted at rest. Security-critical paths — HTML rendering, crypto, OAuth, MIME parsing —
get the most care and the strongest testing (including mutation testing for the core).

### P7. Solo-dev scope discipline
One developer ships the product. Cut anything that is not table-stakes or a core
differentiator. The architecture must not *foreclose* deferred features (e.g. the schema is
account-scoped from day one so unified inbox can be added later), but we do not *build*
deferred features now. Shipping beats completeness.

### P8. Spec before build
We are spec-driven. No code is written without an agreed spec for it. Milestones and their
slices are defined in `roadmap.md`; the **slice** is the unit of planning and build. The chain
is: `constitution` → `vision` → `roadmap` → per-slice **spec** → per-slice **plan** →
per-slice **tasks** → build. Decisions and architecture that **span slices** are recorded as
**ADRs** (`docs/adr/`) and referenced by the slice specs that depend on them — they are not
duplicated into each slice.

For each slice we proceed **spec → plan → tasks**:
- **spec** defines *what* the slice delivers (and its acceptance criteria where applicable),
- **plan** defines *how* it is built,
- **tasks** breaks the plan into concrete to-dos and **always records what is done vs. what
  is still to do**, derived from the spec and plan.

The tasks document is kept current so the work can be picked up at any time — it is the
hand-off and status surface (e.g. to load in planning mode). Each slice is small,
end-to-end, and verifiable. If reality contradicts the spec or plan mid-build, we stop and
amend them — we do not silently drift.

### P9. Adhere to defined style & implementation guidelines
We hold to a single, written set of style and implementation guidelines — naming, error
handling, module/crate boundaries, testing conventions, async patterns, dependency policy,
documentation. New code matches the surrounding code and these guidelines. They live in
`guidelines.md`. UI-framework-specific conventions in that document remain provisional
until the framework is committed in M0.

### P10. Every slice ships complete
A slice is not done when the code runs — it is done when it is **verified, documented, and
reviewed.** Following the per-slice build workflow in `guidelines.md`, each slice delivers,
in order:
1. the **source code** (per its spec and plan);
2. **test cases** that verify it and double as **executable acceptance criteria for the user
   stories** in the slice's spec;
3. a **green run** of those tests (plus fmt, clippy `-D warnings`, and mutation testing on
   touched core crates);
4. an update to the **extensive end-user manual** for any user-facing behavior;
5. an update to the **extensive technical documentation** (in addition to in-code doc comments);
6. a **code review** of the slice's diff;

and only then is it merged via PR (P8, `guidelines.md` §11–12). Documentation is written per
slice and accumulates — never deferred to a "docs later" phase.

---

## How this drives architecture (consequences, not extra rules)

- Local-first (P1) ⇒ UI reads/writes the local store only; sync is a background concern.
- Effortless setup (P3, P5) ⇒ first sync is **newest-first and progressive**: show recent
  mail within seconds, backfill the rest quietly.
- Multi-account ⇒ the storage schema and sync scheduler are **account-scoped from the first
  line of backend code**.
- Lean-and-Rust (P4) ⇒ the shell is the OS webview via **Tauri**, the UI is **Leptos** (Rust →
  WASM, so the app stays Rust end to end with no npm), and every message renders inside a
  script-free, CSP-locked sandbox (ADR-0012, amended in M9 — originally Slint, ADR-0001).
- Integrity (P6) ⇒ the IMAP sync engine and local store are the make-or-break core: their
  schema and sync model are designed up front, proven by a thin end-to-end vertical slice
  (M1), then hardened (M2) before breadth is built on them. We build **value-first** — a
  usable read path early — rather than completing a headless engine before any UI exists.
