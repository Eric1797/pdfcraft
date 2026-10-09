# 02 — Combine grid view: drag cards to reorder

## Problem
The Combine files grid view advertises drag-to-reorder (same list the table
shows), and the drop path (`RowAction::Drop`) plus the
`combine_grid_cards_drag_to_reorder` headless test exist — but dragging a card
did nothing.

## Root cause
`file_card` in `crates/ui-egui/src/combine_ui.rs` allocated its rect with
`Sense::click()`, so `resp.drag_started()` in `grid()` could never fire. The
reorder machinery was intact; only the input sense was wrong.

## Fix
- `crates/ui-egui/src/combine_ui.rs` — `file_card` now uses
  `Sense::click_and_drag()` (click, double-click-to-open and selection are
  unaffected; `click_and_drag` is a superset of click).

## Scope (for upstreaming to `main`)
- `crates/ui-egui/src/combine_ui.rs` only (one line + comment).

## Tests / evidence
- `combine_grid_cards_drag_to_reorder` (`crates/ui-egui/tests/editing.rs`):
  drags `one.pdf` onto `three.pdf`, expects order
  `two, three, one`.
- `combine_grid_single_click_selects_and_double_click_opens` guards the
  click/double-click behavior that shares the same response.

## Suggested upstream commit message
`Fix combine grid cards drag-to-reorder (click_and_drag sense)`
