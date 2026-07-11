# S9.1 — Tauri shell + Leptos scaffold + IPC seam

**Milestone:** M9 (UI rebuild). **Decision:** [ADR-0012](../../../docs/adr/0012-tauri-shell-with-leptos-ui.md).
**Constitution:** P4 (as amended in M9 — lean, Rust, measured), P1 (local-first, UI never waits on
the network), P3 (calm and fast), P8 (spec before build).

## What this slice delivers

The **foundation** of the new UI: a Tauri window running a Leptos (Rust → WASM) frontend that reads
the **real encrypted store** over a typed IPC seam, and paints the three-pane chrome — folder rail,
message list, reading pane.

It is deliberately a *skeleton with real data*, not a feature. **No message rendering** — that is
S9.2, the slice this milestone exists for. Selecting a message shows its stored plaintext only.

The existing Slint app (`geleit-app`) **keeps working and keeps shipping** throughout M9. The new UI
is built alongside it and only replaces it at S9.7's teardown. Every slice leaves the project working.

## Why this shape

- **The IPC seam is the whole architectural bet.** `geleit-{core,platform,store,engine}` are
  UI-agnostic and stay untouched (ADR-0003). The new UI must reach them through a *typed, narrow*
  command surface — not by reaching into the store from view code. Getting that seam right here is
  what makes S9.2–S9.6 mechanical rather than exploratory.
- **Skeleton paint is a P3/P4 requirement, not polish.** WebKit takes ~630 ms to boot before a single
  line of our code runs (measured; `docs/technical/tauri-webkit-spike.md`). A blank window for that
  long reads as a broken app. The shell's `index.html` therefore paints the three-pane chrome
  **statically, before WASM loads**, so the window is never empty.

## User stories / acceptance criteria

| | Story | Acceptance |
|---|---|---|
| **S9.1-1** | The app opens and I see my mail app, not a blank box. | The window paints the three-pane chrome **immediately** — before the WASM frontend has loaded. No blank/white flash. |
| **S9.1-2** | I see my folders. | The rail lists the current account's real folders from the encrypted store, Inbox first (existing `folder_rank` order). |
| **S9.1-3** | I see my messages. | Selecting a folder lists its real messages newest-first: sender, subject, snippet, date, unread dot, star, attachment marker. |
| **S9.1-4** | I can open a message. | Clicking a row opens it in the reading pane showing subject, sender, date, and the **stored plaintext** body. (HTML → S9.2.) |
| **S9.1-5** | It looks like GeleitMail. | Colors, type, spacing, and density follow `design.md`'s token table, in **both** light and dark. |
| **S9.1-6** | It stays fast. | The UI thread never blocks on the store: every command is `async` and the list renders from local data only (P1). |

## Out of scope (named, so they are deferrals and not gaps)

HTML rendering + the sandboxed iframe (**S9.2**), virtualization + threading + flags (S9.3),
refresh/sync (S9.4), compose (S9.5), search/settings/accounts (S9.6), removing Slint/Blitz (S9.7),
CI perf budgets (S9.8). Account setup is out of scope: S9.1 reads whatever account already exists
and shows a calm empty state if there is none.

## Non-functional

- **No npm.** Rust → WASM only; `cargo` and `deny.toml` continue to cover the entire tree.
- **Security posture is set here, before any mail is rendered:** the Tauri CSP forbids remote loads
  app-wide, and the webview's network context is **ephemeral** (no cookie jar, no persistent cache),
  so nothing can be correlated across sessions once S9.2 starts loading images on request.
- The store is opened **encrypted** (SQLCipher, key from the OS keychain), exactly as today.
