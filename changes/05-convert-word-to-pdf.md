# 05 — Convert Word (.docx) to PDF

## What
- New Convert ▸ Word tool ("Word document (.docx)", command `create.word`): pick
  Word files; one opens as a new PDF tab, several are converted and combined in
  order into one PDF.
- New Windows Explorer context menu "Convert to PDF with PdfCraft…" on `.docx`
  and `.docm` (Player model + single-instance handoff, like the Combine verb):
  one file opens as a tab, several combine into one PDF. New `--convert-word`
  launch flag backs it.
- New engine path `Session::create_from_office` / `convert_to_pdf` with a new
  `SourceKind::Office`; `.docx/.docm/.dotx/.dotm` join `CONVERTIBLE`, so Open,
  drag-and-drop, Create ▸ Multiple files, Insert pages and the `doc_create*`
  automation tools accept Word files. Garbage still fails cleanly (never a
  crash, never silent).
- Pure-Rust converter (`crates/create/src/office.rs`, only `flate2` and
  `quick-xml` added to `pdfcraft-create`): paragraphs, headings, bold/italic/
  underline/size/colour, alignment, lists (bullets, decimal, letters, roman),
  simple tables with borders, inline images, section page size and margins.
  Left out (documented in the module docs): headers, footers, footnotes, text
  boxes, shapes, charts, equations, tracked deletions, merged/nested table
  structure, RTL ordering; every font maps to the Helvetica family.
- Also fixes the `clippy::collapsible_if` error in `palette.rs` that was
  failing CI (`crates/ui-egui/src/palette.rs:68`).

## Scope (for upstreaming to `main`)
- `crates/create/Cargo.toml`, `Cargo.lock` (edges only, no new packages)
- `crates/create/src/office.rs` (new), `crates/create/src/lib.rs`,
  `crates/create/src/tests.rs`
- `crates/engine/src/lib.rs`, `crates/engine/src/tests.rs`,
  `crates/engine/src/catalog.rs`, `crates/engine/src/commands.rs`
- `crates/ui-egui/src/{commands,create_ui,create_multiple_ui,files,lib}.rs`,
  `crates/ui-egui/src/single_instance.rs`
- `crates/ui-egui/tests/create.rs`, `crates/ui-egui/tests/links.rs` (untouched)
- `crates/automation/src/tools.rs`, `crates/automation/tests/automation.rs`
- `crates/ui-egui/src/i18n/{ja,es,te,zh-hans,fr,ru}.tsv`
- `apps/pdfcraft/src/main.rs`, `packaging/windows/pdfcraft.wxs`,
  `packaging/windows/test-msi.ps1`
- `parity/acrobat-features.toml` (`create.from-word`)

## Tests / evidence
- `crates/create/src/office.rs::headings_bold_and_text_convert`,
  `::deflated_parts_convert`, `::lists_tables_and_images_convert`,
  `::garbage_is_rejected`, `::text_helpers`
- `crates/automation/tests/automation.rs::creating_from_word_files_through_tools`
- `crates/ui-egui/tests/create.rs::opening_word_documents_converts_them_to_new_pdfs`,
  `::multiple_word_files_combine_into_one_pdf`
- Updated stale refusal assertions (garbage `.docx` now fails as Word, not as
  an unknown type): engine `files_that_cannot_be_converted_are_refused_clearly`,
  automation combine/separate refusal messages, create `source_kind` test.

## Suggested upstream commit message
`Convert Word (.docx) to PDF: tool, shell verb and engine support`
