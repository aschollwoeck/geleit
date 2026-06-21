# S3.6 — HTML fidelity + perf · Plan (the HOW)

## geleit-engine::safehtml (rewrite)
- `formatting_builder()`: ammonia with url_schemes {mailto,cid,http,https,data}, url_relative Deny,
  `add_tags([style,font,center])` + `rm_clean_content_tags([style])` (keep CSS in `<style>`),
  `add_generic_attributes(PRESENTATION_ATTRS)` (style/class/bgcolor/align/width/…).
- `sanitize_html`: formatting_builder + `attribute_filter` stripping `img@src` when `is_remote_url`
  (anything not cid:/data:). `sanitize_html_allowing_remote`: same, no img filter (PRIV-2 opt-in).
- `document()` unchanged: `default-src 'none'` CSP is the network boundary; img-src relaxed only on
  opt-in; scripts never allowed. Doc comment explains the layered model.

## geleit-app
- `ensure_webview(ui, view)`: build the child webview once, hidden, pre-load `document("", false)`
  (cream bg → no black flash). Called from a startup SingleShot timer + lazily by `show_html`.
- `show_html` just loads/positions/reveals.

## Build
- root `[profile.release]` strip = true, lto = "thin".

## Verify
gates; engine tests (fidelity + remote-block + cue); mutants 0-missed; release size; launch +
maintainer eyeball of real formatted mail + first-open snappiness.
