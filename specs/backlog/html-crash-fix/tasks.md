# HTML crash fix · Tasks
- [x] reproduce: narrow window + wide list + open HTML → X BadValue crash (old binary)
- [x] body_rect → body_geom + MIN_READING_W clamp (left never collapses the reading pane)
- [x] body_too_small guard in show_html + the reposition pump (hide instead of degenerate set_bounds)
- [x] match the clamp on the UI list element (webview stays aligned)
- [x] verify hardened binary survives narrow + wide; gates green
- [ ] PR merged
