# S5.6 — Multi-select + bulk · Tasks
## Build
- [x] MessageItem.selected + per-row checkbox (separate hit area) + bulk bar (selected-count)
- [x] selected_ids state; flip_selected_row; bulk_move / bulk_star helpers
- [x] handlers: toggle-select / clear-selection / bulk-archive / bulk-trash / bulk-star; reset on folder switch
## Verify
- [x] AC1 build/test/clippy(+dangerous-tls)/fmt/deny green
- [~] AC2 select + bulk Archive/Delete/Star + Clear — MAINTAINER eyeballs (built on tested move/star)
## Ship
- [ ] tasks all-done; PR merged (completes M5)
