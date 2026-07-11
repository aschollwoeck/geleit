# Performance notes (M8 / S8.3, APP-2 — "calm, fast")

Guiding principle (guidelines §5, [[performance-instantaneous]]): every interaction should *feel*
instantaneous; latency is a defect. Benchmark the **release** build, not debug.

## What keeps it fast
- **Off-UI-thread workers (P1):** all network/blocking work (sync, send, IMAP write-backs, folder
  ops, account setup/removal) runs on `std::thread` workers; only `Send` data + a `Weak<Main>` cross
  back via `invoke_from_event_loop`. The UI thread never blocks on IO.
- **Optimistic actions:** star/archive/trash/move/mark-read/bulk apply to the local store + list
  instantly; the server write-back happens on a worker and self-heals on the next refresh if it fails.
- **Search is synchronous and instant:** FTS5 over the local (encrypted) DB is sub-millisecond at
  personal-mail volume, so search runs on the UI thread per keystroke — no spinner, no worker.
- **Virtualized list:** the message list renders only the visible window (`view::visible_range`), so a
  large folder scrolls smoothly. Listing is capped (1000 rows) and search results (500) to bound work.
- **In-place row updates:** read/star/select toggles patch the single changed row, preserving scroll
  and avoiding a full re-query.
- **Lazy webview:** the HTML viewer (webkit) is built on first use, not at startup — startup stays
  light, and it avoided a GL-coexistence crash.

## Release profile (this slice)
`[profile.release]`: `strip = true`, `lto = "fat"`, `codegen-units = 1`. This cut the binary from
**~32 MB to ~26 MB** and improves runtime via full cross-crate inlining. We deliberately do **not**
set `panic = "abort"`: worker threads rely on `catch_unwind` to contain a panic and report it calmly
rather than taking down the whole app.

## Known follow-ups (not blockers)
- Background sync of non-visible accounts (sync is on-demand per visible account today).
- CONDSTORE/QRESYNC incremental sync (current sync is a recent-window + backfill).
- A real RAM/startup benchmark on the release build is a maintainer step (the harness can't launch
  the GUI here).
