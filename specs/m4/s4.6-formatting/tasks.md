# S4.6 — Basic formatting (Markdown) · Tasks
## Build
- [x] engine: render_markdown (pulldown-cmark) + Draft.html_body + build() multipart + tests
- [x] app: "Format with Markdown" toggle; run_send renders body→html when on
## Verify
- [x] AC1 build/test/clippy/fmt/deny green
- [x] AC2 markdown render + multipart/alternative tested
- [~] AC3 toggle → HTML alternative sent — MAINTAINER eyeballs
- [x] AC4 mutants message 0-missed
## Ship
- [ ] tasks all-done; PR merged (completes M4)
