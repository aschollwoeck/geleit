# S1.8 — Reading pane · Plan (the HOW)

Implements `spec.md`. Slint UI + small store/viewmodel additions; store-only.

## geleit-store
- `set_seen(message_id, bool) -> Result<(), StoreError>`: `UPDATE message SET seen = ?2 WHERE id = ?1`.

## geleit-app::viewmodel (pure, tested)
- `body_display(body: Option<&StoredBody>) -> String`:
  - `None` → "(Body not downloaded yet.)"
  - `Some` with `plain` → the plain text
  - `Some` with only `html` → "(HTML message — safe rendering arrives in M3.)"
  - `Some` with neither → "(No text content.)"

## geleit-app UI (`main.rs`)
- `MessageItem` gains `id: int`; rows carry it. Properties: `selected-message: int`,
  `selected-unread: bool`, and reading-pane content `r-subject`/`r-sender`/`r-date`/`r-body`.
- Callbacks: `message-selected(MessageItem)` (carries id + display fields), `mark-unread(int)`.
- Row: a `TouchArea` → `message-selected(m)`; when `m.id == selected-message`, background
  `accent-quiet` + a 3px accent **guide edge** (the design.md selection treatment).
- Reading pane: subject (title) · sender · date · body (`r-body`), and a "Mark as unread" link
  (visible when a message is open) → `mark-unread(selected-message)`.
- Wiring (a shared `Cell<i64>` current-folder-id so reloads know the folder):
  - `message-selected(item)`: `selected-message = item.id`; load `store.body_for(id)` →
    `body_display` → `r-body`; set `r-subject/r-sender/r-date` from the item; if it was unread,
    `set_seen(id, true)` then **reload the folder's list** (clears the dot); `selected-unread=false`.
  - `mark-unread(id)`: `set_seen(id, false)`; reload the list; `selected-unread=true`.

## Tests
- store: `set_seen` flips `seen` (insert message seen=false → set true → reflected; → false again).
- viewmodel: `body_display` for each of the four cases.
- App launches against a seeded store with no error (manual, like S1.7).

## Verify
`cargo build/test --workspace`, `clippy -D warnings`, `fmt`, `cargo deny check`, `cargo mutants`.
