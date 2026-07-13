# The Tauri shell + Leptos frontend (M9)

How the new UI is put together, and the handful of things that will bite you if you don't know them.
Decision and evidence: [ADR-0012](../adr/0012-tauri-shell-with-leptos-ui.md),
[the webview spike](tauri-webkit-spike.md).

## Shape

```
crates/geleit-app   Tauri host: the window, the OS webview, the IPC seam.        (host binary)
crates/geleit-ui      Leptos frontend: components + pure view logic.               (wasm32 + host)
```

The Slint app was deleted in S9.7 and this crate took over the `geleit-app` name. (During M9's build
it lived alongside Slint as `geleit-shell`; older commits/docs may use that name.)

`geleit-{core,platform,store,engine}` are untouched.

## Two boundaries, both machine-checked

`scripts/check-boundary.sh` (run in CI) enforces:

1. No engine crate depends on a UI crate (ADR-0003).
2. **`geleit-ui` depends on none of our crates at all.** It reaches the engine *only* over the IPC
   seam. This is the one that actually bites: nothing in Cargo stops a Leptos component from
   `use geleit_store::…` and querying SQLite straight from view code, and the moment one does, the
   seam is decorative. So it is asserted, not hoped for.

## The IPC seam

`geleit-app/src/ipc.rs` holds the commands; `dto.rs` holds the types and the pure mapping.

- **DTOs, not store types.** The frontend never sees `geleit_store` types, so the schema can evolve
  without breaking the UI.
- **Every command is `async` and hops to a blocking thread** (`spawn_blocking`). SQLite calls block;
  the webview's event loop must not (P1).
- `Store` is not `Sync`, so app state holds only `db_path + Arc<dyn SecretStore>` and each command
  opens the store. SQLCipher open is ~a millisecond — nothing next to the ~630 ms the webview spends
  booting — and it keeps the commands independent and thread-safe. If it ever shows up in a profile,
  a thread-local or a connection pool is the fix; do not reach for a global `Mutex<Store>`, which
  would serialize the UI behind the slowest query.
- `open_message` already carries `html` (unrendered). S9.2 adds the iframe **without changing the
  seam**.

## Frontend build — no npm, no bundler, no Node

```
scripts/build-ui.sh [--release]
  cargo build -p geleit-ui --target wasm32-unknown-unknown
  wasm-bindgen --target web  →  crates/geleit-app/dist/pkg/
```

That is the whole toolchain. It is what keeps `cargo` and `deny.toml` covering the project's
*entire* dependency tree.

**Gotchas, both of which cost me time:**

1. **`wasm-bindgen` the CLI must exactly match `wasm-bindgen` the crate.** A mismatch fails at
   runtime with an opaque error. `build-ui.sh` reads the version out of `Cargo.lock` and refuses to
   run if the CLI disagrees. CI pins the same version. (The crate version is pinned by `js-sys`, so
   the CLI follows the lockfile, not the other way round.)
2. **Tauri embeds `dist/` into the binary at compile time.** Rebuilding the wasm alone changes
   nothing you can see — you must rebuild `geleit-app` afterwards:
   ```
   ./scripts/build-ui.sh --release && cargo build -p geleit-app
   ```

## No inline scripts — and you cannot use them

`index.html` loads `/early.js` and `/boot.js` as files. Do not "simplify" these back into inline
`<script>` blocks:

- **Tauri's CSP nonce injection does not reach inline *module* scripts.** An inline
  `<script type="module">` silently never runs — the app sits on its skeleton forever, which looks
  exactly like a hang. This wasted a debugging cycle; the fix is to keep scripts external.
- External files also let the CSP stay at a strict `script-src 'self'` with no `'unsafe-inline'`.

## Skeleton paint (constitution P3/P4 — a requirement, not polish)

WebKit spends **~630 ms** spawning its web process before a single line of our code runs. So
`index.html` paints the three-pane chrome as static HTML the moment the document parses, and Leptos
replaces `#app` when it mounts. A blank window for two thirds of a second reads as a broken app.

`boot.js` also renders the failure: if the wasm can't load, the user gets a message, not a frozen
skeleton.

## Theme

The **store** is the source of truth (the same `setting` row the Slint app writes), so a user's
choice survives the migration. But `index.html` cannot await IPC and still paint instantly, so it
paints an *optimistic* theme from `localStorage` (falling back to `prefers-color-scheme`), and the
app reconciles against the store on mount, refreshing `localStorage` for next launch.

## Security posture (set here; relied on by S9.2)

- CSP forbids every remote origin: `default-src 'self'; … img-src 'self' data:; frame-src 'none'`.
- `'wasm-unsafe-eval'` is needed to instantiate *our own* wasm. It does not permit `eval` of remote
  script, and mail never runs in this document.
- `style-src` is `'self'` — **no `'unsafe-inline'`**. Nothing in the app uses inline styles, and
  leaving the directive open would have been a standing weakening bought for nothing.

> ### ⚠️ Read this before writing S9.2
>
> A `srcdoc` iframe **inherits the embedding document's CSP.** So mail rendered via `srcdoc` would
> inherit the strict app CSP above — and since it has no `style-src 'unsafe-inline'`, **every
> message's own inline styles would be silently blocked and all mail would render unstyled.** The
> webview spike didn't hit this because it ran the mail as a standalone page with its own CSP.
>
> The fix is *not* to loosen the app's CSP. Serve the message from its **own origin** — a custom
> protocol (e.g. `mail://…`) registered on the webview — so it carries the CSP that
> `safehtml::document()` already emits (`default-src 'none'; img-src data: cid:;
> style-src 'unsafe-inline'; …`) and inherits nothing from the shell. That also keeps the
> opt-in remote-image path (PRIV-2) a per-message CSP decision, exactly as ADR-0012 describes.
>
> **This is what S9.2 did** — see "The reading pane" below.

## The reading pane (S9.2)

A formatted message is **served from its own `mail://` origin**, never `srcdoc` (for the reason
above). The frontend only ever points an `<iframe>` at `mail://localhost/<id>`; the message body
**never enters the app's document, not even as a string** — `open_message` returns `is_html`/
`has_remote` flags, not the HTML.

Three independent layers, each proven to hold alone with the sanitizer switched *off*
(`tauri-webkit-spike.md`), and re-verified in-app here against a hostile `.eml`:

1. **Sanitizer** — `ammonia` (`safehtml::sanitize_html`), run in `mailproto::message_html`.
2. **Sandbox** — the iframe is `sandbox="allow-popups allow-popups-to-escape-sandbox"`: **no
   `allow-scripts`, no `allow-same-origin`**. Mail can't run code, reach the shell's DOM, touch the
   IPC bridge, or read files.
3. **CSP** — `safehtml::webview_document` emits `default-src 'none'; img-src data: cid:; …`, and
   `mailproto` sends the *identical* policy as a response header too (they must never diverge — a
   test enforces it). `img-src` is the only directive ever relaxed, on explicit opt-in.

**"Load images" (PRIV-2) is a CSP relaxation, not a fetch.** Opting in re-points the iframe at
`mail://localhost/<id>?images=1`; the handler re-serves that one message with `img-src` widened to
**`https:` only** (never cleartext `http:`) and WebKit fetches. It is strictly **per message** — the frontend resets the opt-in on every
`open`, so one click never turns remote loading on for the next message. There is **no HTTP client**.

**Links** never navigate the app. `<base target="_blank">` turns a click into a new-window request;
`main::allow_navigation` refuses everything but our own origins and hands `http(s)`/`mailto` to the
system browser via `xdg-open` (a subprocess — no capability, no HTTP client). The window is built in
`setup()` rather than `tauri.conf.json` precisely because the navigation guard can only be attached
at build time.

**`webview_document` is separate from `document` on purpose.** The old `document()` carries two Blitz
workarounds — `border-collapse:separate!important` (which is *actively wrong* for a real engine) and
`add_font_fallbacks`. S9.2 added a clean
`webview_document` beside it rather than touching `document`. S9.7 deleted `document()` and both workarounds.
- Webview network context is **`incognito: true`** — no cookie jar, no persistent cache — so image
  loads (once S9.2 allows them on request) cannot be correlated across sessions.
- No Tauri plugins are enabled. There is no filesystem, shell, or HTTP capability to grant.

## Frontend interactions (the "Soft daylight" client)

`app.rs` is one `App()` component with one inline `view!`; signals + handlers capture directly (no
callback threading). Notable models:

- **Deferred-commit Undo (archive/delete/spam).** The action does *not* hit the server immediately:
  the row is hidden (`pending: Option<PendingMove>`, filtered out of the list) and an "Undo" toast is
  shown; the server move runs only when the toast window elapses. So **Undo is a pure local cancel
  that can never lose mail**. A new action, or a confirmation toast, commits any queued move first; a
  failed commit restores just the one affected row.
- **Merged "All inboxes".** `store::messages_in_all_inboxes` (every account's `INBOX`, newest first,
  each row tagged with its `account_id`) behind the `list_all_messages` / `search_all` IPC commands;
  `MessageDto.account` carries the tag. In the merged view the folder list is hidden and opening a
  message adopts its account (so a reply sends from the right mailbox).
- **Read/unread** is tracked in two small session sets (`read_now`, `marked_unread`) rather than by
  mutating the list, so a toggle doesn't clone it. `open_message(mark_read)` gates whether opening
  persists the seen flag, honouring the General setting.
- **Star** toggles `Message.flagged` (optimistic local flip + the existing `set_star` server
  write-back). The list-row ★ reads the flag from the loaded list; the reading-pane button uses an
  `open_flagged` state captured when the message opens, so it stays correct even after the message
  leaves the list (e.g. clearing a search) — the body DTO doesn't carry the flag. **Esc** closes the
  search box.
- **Trash (permanent delete)** — two irreversible IPC commands guard a **danger-confirm dialog**
  (`trash_ask: Option<TrashAsk>`), never an undo toast. `empty_trash(account)` resolves the account's
  Trash (`resolve_folder`), empties it server-side (`run_empty_folder`), then clears the local rows
  (`store::delete_folder_messages`) — server first, so a server failure keeps the rows. `delete_forever(id)`
  looks up `message_location` + `account_for_message`, calls `run_delete_permanently` by uid, then
  `delete_message` locally (a message with no server location skips the server step but is still
  removed locally, so the delete never silently reappears). An `in_trash` derived flag
  (selected folder's role is `trash`) reveals the Empty-Trash header button and turns the reading-pane
  **Delete** into **Delete forever** (button and `#` shortcut route to the confirm dialog instead of
  the move-to-Trash undo flow).
- **Compose** — To/Cc are removable chips (`split_addrs`/`merge_addrs`, case-insensitive dedup at both
  chip-commit and send); attachments come from a native picker (`pick_files` shells out to
  zenity/kdialog — deliberately not an in-process GTK dialog, which would clash with the webview loop)
  and are read + size-capped backend-side.
- **Address autocomplete** — each `recipient_field` runs an `Effect` on its input text that calls the
  `suggest_addresses` IPC command (thin wrapper over `store::suggest_addresses`: distinct past
  `from_addr` values, prefix-matched, LIKE-escaped, capped at 6). A pure `rank_suggestions` (`view.rs`,
  unit-tested) drops addresses already chipped on the field before the dropdown renders. Selection is
  on `mousedown` with `preventDefault` so it fires *before* the input's blur-commit and wins over the
  half-typed text; a stale-lookup guard (`input` unchanged after the await) prevents an old response
  clobbering newer keystrokes. `Esc` closes the dropdown before it can reach the composer-discard path.
- **Markdown compose** — a footer toggle (`md_on`, off per new draft) threads a `markdown` flag through
  `api::send_message` → the `send_message` IPC → `run_send`, which renders the body with
  `message::render_markdown` (pulldown-cmark, engine-side) into a `multipart/alternative` (text + HTML).
  The raw body is always the text/plain part, so a non-HTML reader still gets readable text.
- **Drafts** — local-only (SEND-5), over the store's existing `draft` table via four IPC commands
  (`save_draft` / `list_drafts` / `load_draft` / `delete_draft`); `DraftContent` ↔ `ComposeDraft` and
  the `DraftSummary` preview are pure maps in `dto.rs` (unit-tested). **Save draft** upserts the form
  (updating the current row when one is being edited) and closes the composer. `current_draft_id` is
  set only when a row is *resumed*, so continued editing targets the same row. A **Drafts** rail entry
  sets `drafts_open`, which
  makes `rows` return empty and swaps the list pane to a `list_drafts` view; clicking a row runs
  `load_draft` back into the composer with `current_draft_id` set, so edits update the same row.
  `send_message` now carries `draft_id`, so `run_send` deletes the draft after a successful send.
  **Attachments** ride along: `save_draft` reads the composer's attachment *paths* into bytes and
  `replace_draft_attachments`; `load_draft` returns a `ResumedDraft` whose bytes are materialised back
  to per-draft temp files (`materialize_draft_attachments`, unit-tested, sanitised names in numbered
  sub-dirs so the chip basename stays clean) so send / re-save use the normal path-based flow. Only
  **server-backed drafts** (IMAP `APPEND` to the Drafts folder) remain out of scope.
- **Save/open .eml** (READ-10) — re-wires the surviving engine core, no network. **Save** (reading-pane
  action) → `save_eml(id)`: `export_eml` rebuilds RFC 822 bytes from the stored header + body (faithful
  bodies; MIME reconstructed from parts, not byte-identical), a native save dialog (`pick_save_path`,
  zenity `--save`/kdialog) names it `<safe_filename_stem(subject)>.eml`, then writes. **Open mail
  file…** (rail entry) → `open_eml_file(account)`: pick + read a file, `parse_eml` (mail-parser + the
  same `mime::parse_body` as sync), `upsert_folder(SAVED_FOLDER)` + `upsert_message(uid=None)` +
  `store_body`, return the new id; the UI reloads folders, switches to **Saved**, and opens it. HTML
  can't be rendered ad-hoc (the reading pane serves HTML by store id over `mail://`), so an opened .eml
  becomes a real local row. `safe_filename_stem` is a pure, unit-tested helper in `dto.rs`.
- **Attachments (view + save)** (READ-8) — the reading pane lists attachments (name + `human_size`)
  from the stored metadata (`attachments_for`, populated at sync); `open_message`'s `MessageBodyDto`
  now carries `attachments: Vec<AttachmentDto>` in parse order, so a chip's index is its save key. The
  **bytes are not stored**, so **Save** fetches on demand: `save_attachment(message_id, index)` →
  `run_fetch_attachment` → new `imap::fetch_raw_message` (whole message by UID via `BODY.PEEK[]`, so
  no `\Seen`) → pure `mime::extract_attachment(raw, index)` → the existing `pick_save_path` + write.
  `human_size` and `safe_attachment_filename` (path-traversal-safe default name) are pure, unit-tested
  helpers in `dto.rs`; `extract_attachment` is unit-tested in `mime.rs` and the fetch path has a live
  `#[ignore]` Dovecot test (`live_fetch_raw_and_extract_attachment`). This is the one slice with a new
  network path — everything else reuses stored data.
- **Folder management** (ORG-6) — the IMAP primitives (`imap::create_folder`/`rename_folder`/
  `delete_folder`) already existed; this wires them up. `create_folder`/`rename_folder`/`delete_folder`
  IPC commands run the server op on a worker (`run_*_folder`), then update the local store: create →
  `upsert_folder`, **rename → `store::rename_folder` (an in-place `UPDATE`, so `folder_id` is stable
  and the folder's messages stay attached** — a delete+re-list would cascade them away), delete →
  `store::delete_folder` (the row's `ON DELETE CASCADE` removes its messages/bodies/attachments).
  Protected folders (Inbox, the role folders, local `Saved`/`Drafts`) are gated by the pure
  `is_protected_folder` — mirrored in `view.rs` (hides the rail's Rename/Delete affordances) and
  `dto.rs` (the IPC re-checks the authoritative copy, so a protected rename/delete is refused even if
  the UI is bypassed). `validate_folder_name` (pure) trims + rejects blank/slashed names. The rail
  gains a "+ New folder" button and a per-folder ⋯ menu; delete goes through a danger-confirm dialog.
  Store `rename_folder`/`delete_folder` are unit-tested; the create/rename/delete round-trip has a live
  `#[ignore]` Dovecot test (`live_create_rename_delete_folder`). `GELEIT_FOLDER=new|menu` is a seam.
- **Multi-select bulk actions** (ORG-7) — pure UI reusing the per-message commands, no new IPC. A
  `selected: HashSet<i64>` (mirrors `read_now`/`marked_unread`) drives a hover-revealed per-row
  checkbox and a bulk bar (Archive / Delete / Mark unread / Clear + a select-all box backed by the pure
  `all_selected` in `view.rs`). **Shift-click** extends a range from the last plain-clicked row (the
  `select_anchor`) via the pure `range_ids(ordered, anchor, target)` helper over the current message
  order. Bulk Archive/Delete reuse the deferred-Undo machinery, generalized from a single
  `PendingMove{id}` to `PendingMove{ids}`: `rows()` hides the whole set, one "N archived · Undo" toast
  shows, and on commit the server moves loop `move_to_role` per id, re-inserting only the rows that
  fail. Mark read / Mark unread are immediate `set_read`/`set_unread` loops (both share
  `set_seen_and_writeback` — local seen flag + `\Seen` write-back). Selection clears on
  folder/account/view/search change.
- **List** is one keyed `<For>` over three fixed day buckets (Today/Yesterday/Earlier), so rank-ordered
  search results group correctly. Reading-pane header order is actions · sender · subject (buttons
  pinned on top). Keyboard: `c` `/` `e` `#` `r` `f` `z` `j`/`k` `Esc`.

## Testing

The frontend is split so that the parts worth testing *are* testable without a browser:

| | |
|---|---|
| `geleit-ui/src/view.rs` | Pure display logic — dates, elision, `nav_index` (keyboard-nav index math), `split_addrs`/`merge_addrs` (recipient-chip parsing + case-insensitive dedup). Unit + **mutation** tested on host. |
| `geleit-app/src/dto.rs` | Pure store→UI mapping, folder ordering. Unit + **mutation** tested. |
| `app.rs`, `api.rs`, `ipc.rs` | View declaration and glue — excluded from mutants (survivors there are spurious), the same split as `geleit-app`'s `main.rs`/`viewmodel.rs`. |

`geleit-ui` compiles for the **host** as well as wasm (`crate-type = ["cdylib", "rlib"]`, wasm
entrypoint behind `cfg(target_arch = "wasm32")`) — that is what lets clippy and `cargo test` cover it
like any other crate. CI *also* builds it for wasm, so a wasm-only break can't slip through.

## Screenshot verification

The build environment can't inject clicks (no `xdotool`), so **debug-only** env seams drive the UI
into a state on boot. Each `dev_*` command is `#[cfg(debug_assertions)]`, so in a release build it
isn't registered at all and the env var is never read:

| Env var | Opens |
|---|---|
| `GELEIT_OPEN=<id>` | that message in the reading pane (`GELEIT_IMAGES=1` loads its remote content) |
| `GELEIT_COMPOSE=new\|reply\|reply_all\|forward` | the composer (`reply`/`reply_all`/`forward` also need `GELEIT_OPEN`) |
| `GELEIT_TO=<text>` | with `GELEIT_COMPOSE=new`, pre-fills the To input (surfaces the autocomplete dropdown) |
| `GELEIT_DRAFTS=1` | the saved-Drafts list |
| `GELEIT_RESUME=1` | resumes the newest draft (composer with its content + attachments) |
| `GELEIT_SELECT=<id,id,…>` | pre-selects those message rows (surfaces the bulk-action bar) |
| `GELEIT_FOLDER=new\|menu` | the New-folder dialog, or the first user folder's ⋯ (Rename/Delete) menu |
| `GELEIT_UNIFIED=1` | the merged "All inboxes" view |
| `GELEIT_SETUP=1` | the add-account wizard |
| `GELEIT_SETTINGS=1` | the Settings window |
| `GELEIT_SEARCH=<query>` | search, opened and run |
| `GELEIT_TRASH=empty\|delete` | the irreversible-delete confirm dialog (Empty Trash / Delete forever) |

The seam waits for boot to finish (`loaded`) before firing, so preferences (e.g. mark-as-read) are
already in effect. Seed a demo DB with `cargo run -p geleit-app --example seed_demo` (`-- --dark`
for dark). **Discipline: never screenshot against a real account, and target your *own* window by
PID** (`pgrep -nf target/debug/geleit-app` → `wmctrl -lp`) — a maintainer's release instance may be
running the same window title.
