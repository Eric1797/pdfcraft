# 04 — Home community card opt-in, recent files capped at 10

## New behavior
- The "Join the ArtCraft community" card on Home is hidden by default. It can
  be turned back on in Preferences ▸ Tools and Home ("Show the ArtCraft
  community box on Home", default off). The Help menu, About dialog and
  palette links are untouched — only the Home card is gated.
- Recent files keep at most **10** entries (was 12). Home and File ▸ Open
  Recent list whatever is kept, most recent first.

## Scope (for upstreaming to `main`)
- `crates/ui-egui/src/lib.rs` — `show_community` field, default,
  persist/restore, `set_option("show-community", …)`, `truncate(10)`.
- `crates/ui-egui/src/home.rs` — community card gated on `show_community`.
- `crates/ui-egui/src/js_ui.rs` — Preferences checkbox (same "Tools and Home"
  section as change 03).
- `apps/pdfcraft/src/main.rs` — one-line CLI options doc.
- `crates/ui-egui/src/i18n/{fr,ru,zh-hans}.tsv` — shared with change 03.
- `crates/ui-egui/tests/links.rs` — the two Home link tests now opt in with
  `app.show_community = true`.
- `crates/ui-egui/tests/ui.rs` — new tests `the_community_card_is_opt_in`,
  `tools_and_home_preferences_persist_and_have_options`,
  `recent_files_keep_at_most_ten`.

## Tests / evidence
- Updated: `discord_link_on_the_home_screen_opens_discord`,
  `home_screen_links` (`crates/ui-egui/tests/links.rs`).
- New tests listed above (`crates/ui-egui/tests/ui.rs`).

## Suggested upstream commit message
`Make the Home community card opt-in; cap recents at ten`
