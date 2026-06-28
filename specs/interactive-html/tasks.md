# Interactive HTML · Tasks
- [x] htmlrender::HtmlView (open/resize/render-viewport/scroll/pointer-selection/link) replacing tiles
- [x] app: html_doc = HtmlView; show_html opens + renders viewport; scroll/pointer/copy/link handlers
- [x] Slint: viewport Image + TouchArea (pointer + scroll-event) + scrollbar; Ctrl+C copy
- [x] clipboard via wl-copy/xclip/xsel subprocess
- [x] viewport height computed in Rust (changed-callback recurses Slint layout — avoided)
- [x] re-render on splitter-released (width change)
- [x] verify in-app: render + scroll (scrollbar moves) + drag-select highlight (GELEIT_SCROLL/SELECT)
- [ ] gates green → PR merged
- [~] follow-ups: window-resize re-render, scrollbar grab-from-thumb, copy keybinding on Wayland,
      exact body-height measurement, per-frame paint perf tuning for very large emails
