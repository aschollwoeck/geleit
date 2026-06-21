# S4.4 — Compose window + send · Plan (the HOW)

- engine: `imap::password(secrets, username)` reads the stored credential (shared with SMTP).
- refresh: `parse_addrs` (split on `,`/`;`, trim, drop empties — pure, unit-tested); `run_send`
  loads the first account + `imap_settings` (username/allow_invalid) + `smtp_settings`, reads the
  password, builds an `engine::message::Draft` (from = account email/display name; to/cc parsed),
  `message::build` → bytes, `smtp::envelope` + `smtp::send` over a current-thread runtime. Worker
  thread; calm errors.
- app Slint: `composing`/`c-*`/`sending`/`compose-status` props + `compose`/`send-message`/
  `cancel-compose` callbacks; a "New message" rail button; a centered overlay card (LineEdits +
  TextEdit body + Send/Cancel). Handlers: `on_compose` hides the webview + clears fields;
  `on_send_message` validates ≥1 recipient, spawns `run_send`, then closes on success / shows the
  error on failure.

## Verify
gates; `parse_addrs` unit test; maintainer eyeballs the overlay + sends a real message.
