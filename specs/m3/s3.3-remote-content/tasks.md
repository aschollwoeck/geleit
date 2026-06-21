# S3.3 — Load remote content + cue · Tasks

## Build
- [x] engine `sanitize_html_allowing_remote` + test
- [x] app: `remote-blocked` prop + `load-remote` callback; cue bar; current-allowed state; body_rect offset
- [x] wire on_message_selected (compute blocked/allowed/diff) + on_load_remote; manual touch

## Verify
- [x] AC1 build/test/clippy/fmt/deny green
- [x] AC2 allowing_remote keeps http(s), strips scripts (tested)
- [x] AC3 cue shows for remote msgs; load re-renders (eyeball)
- [x] AC4 remote loads only on opt-in
- [x] AC5 mutants 0 missed

## Ship
- [x] Code review; tasks all-done; PR merged — completes M3
