# S5.6 — Multi-select + bulk actions (ORG-7) · Spec

Final slice of **M5**. Select multiple messages and act on them together.

## In scope
- App: a per-row **checkbox** (its own hit area, on top of the open-message area) toggles selection;
  `selected-count` drives a **bulk-action bar** (Archive / Delete / Star / Clear) shown while any are
  selected. `bulk_move` / `bulk_star` apply the action to all selected (optimistic + one worker that
  loops the write-back), then clear the selection. Selection resets on folder switch.

## Out of scope
- Shift-range select; bulk move-to-arbitrary-folder (Archive/Trash/Star covered); bulk mark-read;
  permanent bulk delete in Trash (bulk Delete moves to Trash).

## Acceptance criteria
1. build/test/clippy -D warnings (+dangerous-tls)/fmt/`cargo deny check` green.
2. Checkbox toggles selection + count; bar appears; Archive/Delete/Star act on all selected then clear;
   Clear deselects; folder switch resets (maintainer eyeballs; built on tested move/star + find_folder).

## Deliverables
- `MessageItem.selected` + checkbox + bulk bar; `selected_ids` state; `flip_selected_row`,
  `bulk_move`, `bulk_star`; toggle/clear/bulk handlers.
