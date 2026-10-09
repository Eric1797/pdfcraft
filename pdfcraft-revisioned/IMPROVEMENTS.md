# PdfCraft Revisioned — improvement list

Status key: `todo` (accepted, not started) · `doing` · `done` · `later` (deferred with reason).

## Improvements

1. `done` — Organize Pages: at the last `+` gap, add an option to insert a blank page.
   Trailing gap opens a menu (files / blank page sized like the last page); reuses
   existing UI strings so no translation catalog changes were needed. Verified:
   fmt, check, clippy `-D warnings`, new `trailing_gap_inserts_a_blank_page` test
   plus neighbouring gap tests pass locally.
2. `later` — Optimise memory usage for larger PDF files (only if possible; otherwise not now).
   Note: lazy loading is a planned upstream milestone (`core.lazy-loading`, ByteSource);
   files are buffered whole today. Revisit only with profiling evidence.
3. `todo` — MUST. Top file tab bar with many PDFs open: it fills up, so add a horizontal
   scrollbar (plus mouse-wheel scrolling); stop the file tabs colliding with the
   help/Discord/theme icons (tabs go behind them). Also redesign per reference:
   Menu and Home next to the file tabs, window controls integrated into the same
   top bar (Adobe-like layout, own visual design).
4. `todo` — Tab groups for people working with many files: right-click the top tab bar
   to make groups; tabs can be dragged into groups.
5. `todo` — Smoother zoom in/out; when zoomed out there is a visible cut — make it smooth.
6. `todo` — Merge the window minimise/maximise/close buttons into the top file bar
   (custom title bar) and move Menu + Home to the top corner.
