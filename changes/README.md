# Changes on `rev-batch-1` — upstream one by one

Each file in this folder describes one self-contained change currently stacked on
the `rev-batch-1` branch, so it can be committed to the main repo (`main`) one
at a time later. Read the files in number order.

| # | Change | File |
|---|--------|------|
| 1 | Windows custom title-bar buttons clipped past the panel edge | `01-windows-title-bar-buttons.md` |
| 2 | Combine grid view: drag cards to reorder | `02-combine-grid-drag-reorder.md` |
| 3 | Hide unfinished tools, drop the Ready chip, Preferences opt-in | `03-hide-unfinished-tools.md` |
| 4 | Home community card opt-in, recent files capped at 10 | `04-home-community-toggle-and-recent-cap.md` |
| 5 | Convert Word (.docx) to PDF (tool, shell verb, engine) + CI clippy fix | `05-convert-word-to-pdf.md` |

How to upstream one change at a time (example for change 3):

1. Check out (or cherry-pick onto) `main`.
2. Re-apply only the files/sections listed under "Scope" in that change's file.
3. Run the "Tests / evidence" commands listed there.
4. Commit with the suggested message and open the PR.

Nothing in this folder ships in the app; it is notes for the maintainer.
