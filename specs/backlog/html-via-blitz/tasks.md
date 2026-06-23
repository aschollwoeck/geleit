# HTML via Blitz · Tasks
- [x] spike: Blitz + anyrender_vello_cpu render sample HTML → PNG (validate build, API, fidelity)
- [x] cargo deny passes with Servo/Stylo
- [x] htmlrender module: render(html,width,dark)->{image,doc} + link_at(doc,x,y)->href
- [x] reading pane: Image (rendered HTML) + TouchArea→html-click; text fallback otherwise
- [x] message-selected renders HTML to bitmap; remote-blocked cue kept (PRIV-3)
- [x] remove wry + gtk + unstable-winit-030 + all webview code (HtmlView/show/hide/body_rect/gtk pump)
- [x] verify in-app render (screenshot) + small-window no-crash + gates green
- [ ] follow-ups: hi-dpi, resize re-render, PRIV-2 opt-in, catch_unwind fallback
- [ ] PR merged
