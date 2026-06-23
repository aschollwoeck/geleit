# HTML crash #2 · Tasks
- [x] reproduce: small window + open HTML → winit GLXBadWindow panic (winit set_theme flush_requests)
- [x] body_too_small now gates on window (800x540) + body (360x240) size → text fallback when small
- [x] refresh stale "(HTML message — …M3)" fallback text
- [x] verify: tiny/narrow no longer crash; maximized renders HTML; gates green
- [~] flaky large-window GL crashes can't be fully ruled out — MAINTAINER to report if persists
- [ ] PR merged
