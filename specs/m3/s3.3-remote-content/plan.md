# S3.3 — Load remote content + cue · Plan (the HOW)

## geleit-engine::safehtml
- `pub fn sanitize_html_allowing_remote(raw) -> String`: ammonia with url_schemes
  {mailto,cid,http,https} and default (PassThrough) relative URLs — keeps remote refs; still strips
  scripts/handlers + dangerous tags. + test (http img kept; script stripped).

## geleit-app
- `in property <bool> remote-blocked;` + callback `load-remote()`.
- `body_rect(ui)`: top offset 164 when `remote-blocked` (room for the cue), else 132 — single source
  used by show_html + the pump.
- State: `current_allowed: Rc<RefCell<Option<String>>>` = the allowed document for the open message.
- `on_message_selected` (HTML): `blocked=document(sanitize_html(h))`, `allowed=document(allowing(h))`;
  `remote_blocked = blocked != allowed`; store allowed; render blocked.
- cue bar in the reading-pane header (when `remote-blocked` && a message open): "Remote content
  blocked · [Load remote images]" → `load-remote()`.
- `on_load_remote`: set remote-blocked=false; render the stored allowed doc.

## Verify
gates; engine tests; mutants; launch + maintainer eyeball.
