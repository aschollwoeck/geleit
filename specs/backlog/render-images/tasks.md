# Render images · Tasks
- [x] diagnose: Blitz draws no images without a NetProvider (data: included) — verified via spike
- [x] DataUriProvider (offline, data: only) wired into htmlrender::render + multi-pass resolve
- [x] verify inline data: image paints (spike) + content height grows
- [x] remoteimg::inline_remote_images (ureq fetch → data: URIs; caps + image/* only)
- [x] deny.toml: allow ureq only (the single permitted HTTP client), documented
- [x] Load images button + load-remote/apply-loaded-html worker→UI re-render
- [x] verify end-to-end in-app (GELEIT_SHOT=loadimg, local server): banner renders, cue clears
- [x] gates green
- [ ] PR merged
