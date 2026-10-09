# PdfCraft Revisioned — improvement list

Status key: `todo` (accepted, not started) · `doing` · `done` · `later` (deferred with reason).

## Improvements

1. `done` — Organize Pages: at the last `+` gap, add an option to insert a blank page.
   Trailing gap opens a menu (files / blank page sized like the last page); reuses
   existing UI strings so no translation catalog changes were needed. Verified:
   fmt, check, clippy `-D warnings`, new `trailing_gap_inserts_a_blank_page` test
   plus neighbouring gap tests pass locally.
2. `later` — Optimise memory usage for larger PDF files (only if possible; otherwise not now).
   Assessed: document bytes are already `Arc`-shared across engine, renderer and
   automation (no duplicate buffering found); real gains need the upstream
   `core.lazy-loading` milestone (ByteSource) plus profiling. No blind changes made.
3. `doing` — MUST. Top file tab bar with many PDFs open: it fills up, so add a horizontal
   scrollbar (plus mouse-wheel scrolling); stop the file tabs colliding with the
   help/Discord/theme icons (tabs go behind it). Also redesign per reference:
   Menu and Home next to the file tabs, window controls integrated into the same
   top bar (Adobe-like layout, own visual design).
4. `later` — Tab groups (dropped by user for now).
5. `doing` — Smoother zoom in/out; when zoomed out there is a visible cut — make it smooth.
   Implemented as eased zoom-step flights (buttons/keys/menu glide ~140 ms, gestures
   stay instant); needs CI test run for proof.
6. `doing` — Merge the window minimise/maximise/close buttons into the top file bar
   (custom title bar) and move Menu + Home to the top corner. Implemented
   Windows-only (`with_decorations(false)` + own buttons/drag/edge resize);
   Menu/Home were already top-left of the tabs. Needs CI test run for proof.
