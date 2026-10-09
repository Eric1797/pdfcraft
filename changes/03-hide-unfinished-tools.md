# 03 — Hide unfinished tools, drop the Ready chip, Preferences opt-in

## Problem
Every tool row carried a green "Ready" chip, and planned/provider tools were
listed alongside finished ones, suggesting they work.

## New behavior
- The "Ready" chip is gone: finished tools are listed with no chip at all.
- Planned and provider items are hidden from the tool panel **and** the
  command palette by default. Shown items keep their milestone (`M11`, …) or
  `AI` chips when they are visible.
- Filtering is per **item**, not per group: group-level availability is coarse
  (e.g. "Protect a PDF" is marked M8 but almost all its items work), so hiding
  whole groups would remove working tools. All groups stay listed.
- A tool detail that hides items ends with a quiet footer, e.g.
  "3 planned tools are hidden", plus a "Show in Preferences" link that opens
  Preferences.
- The "Coming in milestone M…" group banner no longer claims "Items marked
  Ready work today."
- Preferences ▸ Tools and Home has "Show planned and provider tools"
  (default off). Persisted across restarts; also settable headlessly via
  `--show-planned-tools on|off` and the `ui.set` control call.

## Scope (for upstreaming to `main`)
- `crates/ui-egui/src/lib.rs` — `show_planned_tools` field, default,
  persist/restore, `set_option("show-planned-tools", …)`.
- `crates/ui-egui/src/panels.rs` — item filtering, chip removal, banner text,
  hidden-count footer.
- `crates/ui-egui/src/palette.rs` — same filter for catalogue hits
  (registered commands are always listed on their own merits).
- `crates/ui-egui/src/js_ui.rs` — Preferences checkbox.
- `apps/pdfcraft/src/main.rs` — one-line CLI options doc.
- `crates/ui-egui/src/i18n/{fr,ru,zh-hans}.tsv` — new strings, English until
  translated (required by the `*_covers_ui_literals` tests).
- `crates/ui-egui/tests/ui.rs` — new test
  `unfinished_tools_are_hidden_until_preferences_show_them`.

## Tests / evidence
- `unfinished_tools_are_hidden_until_preferences_show_them`
  (`crates/ui-egui/tests/ui.rs`).
- Existing palette/command tests only use registered commands, unaffected.

## Suggested upstream commit message
`Hide unfinished tools by default, with a Preferences opt-in`
