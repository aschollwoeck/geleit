# Resizable splitter · Tasks
- [x] Slint: list-width prop; 6px splitter handle (ew-resize) with stable-frame drag math; clamp 280–680
- [x] Rust: body_rect left = 240 + list-width + 6 (webview tracks via the 16ms pump)
- [x] persist list-width on release; restore at startup (settings table)
- [x] verified layout + webview tracking at a non-default width (screenshot); gates green
- [~] drag feel/clamps — MAINTAINER eyeball (no mouse-drag injection here)
- [ ] PR merged
