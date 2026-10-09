# 01 — Windows custom title-bar buttons clipped past the panel edge

## Problem
On Windows (custom decoration-less title bar), only the Minimize button was
visible in the tab strip. Close and Maximize/Restore were pushed past the right
panel edge, so they could not be seen or clicked.

## Root cause
`crates/ui-egui/src/chrome.rs`, `tab_strip`: the tab `ScrollArea` reserves
`available_width - reserve` for itself, where `reserve` is measured from the
previous frame. `before = ui.available_width()` was taken **after** the Open
button, so `reserve` covered only the right-side icons and the tabs came out
one Open-button too wide — shoving the rightmost window buttons off the edge.

## Fix
Measure `before` **before** the Open button, so the reserved width covers both
the Open button and the right-side icons:

- `crates/ui-egui/src/chrome.rs` — moved one line plus an explaining comment.

## Scope (for upstreaming to `main`)
- `crates/ui-egui/src/chrome.rs` only.

## Tests / evidence
- Existing headless test `window_buttons_sit_in_the_tab_strip`
  (`crates/ui-egui/tests/ui.rs`, Windows-only) asserts Minimize, Maximize and
  Close are all present.
- Commit: `Fix Windows title-bar buttons clipped past panel edge` (already on
  `rev-batch-1`).

## Suggested upstream commit message
`Fix Windows title-bar buttons clipped past panel edge`
