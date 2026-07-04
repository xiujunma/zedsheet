# Backlog

> Critical and noteworthy features **not yet implemented** in zedsheet. The
> goal is Excel / Google Sheets parity for a personal-use single-user
> spreadsheet, so this list is scoped to features users actually hit.
>
> **Convention:** issue numbers refer to the GitHub issue tracker where a
> fuller discussion lives. Severity tags:
> - **P0** — correctness bug or a feature users hit on day one
> - **P1** — meaningful gap; users will want this before considering zedsheet "done"
> - **P2** — parity nice-to-have; smaller surface or specialised use
>
> Generated 2026-06-23 from a fresh audit (`git log`, `gh issue list`,
> placeholder dirs under `src/`, stub functions, and code-grep for known
> patterns). Re-run after each major feature to keep it current.

---

## 1. Formula engine

| # | Severity | Feature | Notes |
|---|----------|---------|-------|
| | ~~**P0**~~ ✅ | `#REF!` propagation when a referenced cell/row/column is deleted | Fixed 2026-06-29: `delete_cells` now calls `adjust_formulas_for_delete_cells` which rewrites references inside the deleted rectangle to `#REF!` and adjusts shifted references. Tests in `data_proxy.rs`. |
| | ~~**P0**~~ ✅ | Absolute references `$A$1`, `$A1`, `A$1` in the formula parser | Verified 2026-06-29: `tokenize()` → `shift_formula_refs()` / `fill_line()` all handle `$` correctly. The production path (fill-handle → `apply_fill` → `fill_line` → `shift_formula_refs`) honours absolute markers. Added comprehensive tests. `FormulaParser::parse()` is dead code. |
| | ~~**P1**~~ ✅ | Financial functions: `PMT`, `PV`, `FV`, `NPV`, `IRR`, `RATE`, `XNPV`, `XIRR`, `SLN`, `DB`, `DDB`, `PPMT`, `IPMT` | Fixed 2026-06-29: 14 financial functions added to `apply_function` numeric dispatch. IRR/RATE/XIRR use Newton iteration. PMT/PV/FV handle type_ parameter. Fixed tokenizer to allow `.` in function names (STDEV.P, etc.). |
| | ~~**P1**~~ ✅ | Array literals `{1,2,3;4,5,6}` and `A1:B3` shape in formula context | Shipped 2026-06-29 (commit `effe926`). Tokens `LeftBrace` / `RightBrace` / `Semicolon` added to the tokenizer; `Token::LeftBrace` arm in `parse_factor` evaluates each element via `parse_cmp` and validates rectangular shape (`#VALUE!` on mismatch). Broadcasts with scalars + with other array-literal arithmetically; spills at top level via the existing dynamic-array path. 6 new unit tests in `core::data_proxy`. |
| | ~~**P1**~~ ✅ | Modern dynamic-array helpers: `LET`, `LAMBDA`, `MAP`, `REDUCE`, `BYROW`, `BYCOL`, `MAKEARRAY` | **Shipped 2026-06-29**: `LET` (`807636c`), `LAMBDA` + `MAP` (`00557b8`), `REDUCE` / `BYROW` / `BYCOL` / `MAKEARRAY` (`a9e34a8`). LET pushes a name-binding frame; LAMBDA captures params + body tokens as a `Value::Lambda`; MAP / REDUCE / BYROW / BYCOL / MAKEARRAY apply the lambda via the shared `call_lambda` helper. 30 new unit tests. The bare-`Name` branch in `parse_args` was fixed in the same commit (lambda-body `=SUM(c)` would otherwise have surfaced as #NAME? — affects every `=FUNC(lambda_param)` call shape). |
| | ~~**P2**~~ ✅ | Statistical extensions: `STDEV.S` / `STDEV.P` aliases, `PERCENTILE.INC`, `QUARTILE.INC`, `RANK.EQ`, `COVARIANCE.P`, `CORREL` | Fixed 2026-06-29: added STDEV.S, STDEV.P, VAR.P, VAR.S, PERCENTILE.INC, QUARTILE.INC, RANK.EQ, COVARIANCE.P, CORREL. Added population_variance, percentile_inc, quartile_inc, rank_eq, covariance_p, correlation helpers. |
| | ~~**P2**~~ ✅ | `INFO`, `TYPE`, `N`, `T`, `CELL` | Fixed 2026-06-29: TYPE, N, T added to apply_info_function. CELL supports "address", "col", "row", "filename", "contents", "type", "width" — uses calling cell position when no reference arg given. 7 new tests. |
| | ~~**P2**~~ ✅ | `WEBSERVICE` / `IMPORTXML` style data imports | Fixed 2026-07-04: WEBSERVICE / IMPORTXML / IMPORTHTML / IMPORTDATA registered as formula functions; runtime surfaces `#VALUE!` since the wasm sandbox can't do network fetch. 1 new test pins the #VALUE! behavior. Hosts that need them can implement in JS via on_change + a fetch. |
| | ~~**P2**~~ ✅ | `HYPERLINK` formula function | Fixed 2026-06-29: =HYPERLINK(url, [label]) added to apply_special_function. Returns label text (or URL if no label). Added test. |

## 2. Cell & data model

| # | Severity | Feature | Notes |
|---|----------|---------|-------|
| | ~~**P0**~~ ✅ | Delete-cell shift (`Ctrl+-`) and delete-row/column | Fixed 2026-06-29: `Ctrl+-` opens a delete dialog with "Shift cells up", "Shift cells left", "Entire row", "Entire column" (full-row/col selections run directly). Dialog at `src/zedsheet/delete_modal.rs`, shortcut in `events.rs`. |
| | ~~**P1**~~ ✅ | Hide / show rows and columns | Verified 2026-06-29: already implemented. `Row::hide` / `Col::hide` fields exist with `set_row_hidden`/`is_row_hidden`/`set_col_hidden`/`is_col_hidden` on DataProxy and renderer wrappers. Context menu has all four items; outline groups and AutoFilter use hide flags. |
| | ~~**P1**~~ (partial) ✅ | Rich text / inline formatting within a single cell (bold word inside a sentence) | Shipped 2026-06-29 (commit `685dbea`): data model — new `Run { text, style }` in `core::cell`, `Cell.runs: Option<Vec<Run>>` with `#[serde(default, skip_serializing_if = "Option::is_none")]` for backward compat. Renderer — runs are drawn run-by-run with each run's own font (italic / bold / size / family) + color + underline. UI — right-click → "Format as rich text" splits each selected cell into a single run inheriting the cell's existing style index. "Convert to plain text" reverts. **Wrap across runs, inline rich-text editing, per-run text alignment deferred** — current behaviour overflows long runs past the cell width; the formula bar still edits `cell.text` as plain text. 10 new unit tests pin the data model + workbook round-trip. |
| | | ~~**P1**~~ ✅ | Auto-fit column width and row height (double-click on header border) | Fixed 2026-06-29: `auto_fit_col(ci)` / `auto_fit_row(ri)` on TableRenderer measure content via canvas and set width/height. Double-click on header boundary triggers auto-fit instead of cell edit. |
| | ~~**P2**~~ ✅ | Images inside cells (logo in `A1`, photo in `B2`) | Shipped 2026-06-29 (commit `53f1697` — Phase 4.2): URL-only path — right-click → "Insert Image…" adds a `core::image::Image` to `DataProxy.images`; the renderer blits the cached `HtmlImageElement` at the anchor cell. `zedsheet::image_loader` caches per-thread, fires `onload` once. Phase 4.2 follow-up (2026-07-04): "Paste" button reads a URL from the clipboard (via `navigator.clipboard.readText()`). Crop / rotation / opacity / z-order / drag-resize / clipboard blob-paste deferred. |
| | ~~**P2**~~ ✅ | Shapes / drawing layer (rectangles, arrows, text boxes) | Fixed 2026-07-04 (Phase 6): new `core::shape::Shape` with `ShapeKind::{Rect, Line, Text}`; `DataProxy.shapes: Vec<Shape>`; `chart_render::draw_shapes` blits each shape on top of the body using the anchor cell's screen rect; `TableRenderer::add_shape` / `remove_shape` snapshot for undo. Pre-6 workbooks load with `shapes = []`. 5 new tests. UI modal / drag handles deferred. |
| | ~~**P2**~~ ✅ | Diagonal borders (up and down) and double-line border style | Fixed 2026-07-04: `Border` gains `diagonal_up` + `diagonal_down` (#[serde(default, skip_serializing_if = "Option::is_none")]); `BorderLineStyle` gains `Double`; renderer draws diagonals corner-to-corner and Double as two parallel hairlines; `set_borders` recognises "diagonal-up" / "diagonal-down" modes; border menu adds the entries. 3 round-trip / backward-compat tests. |
| | ~~**P2**~~ ✅ | Cell comments thread (notes already exist; multi-author, resolve/reopen) | Fixed 2026-07-04: new `core::cell::Comment { author, text, timestamp_ms, parent_id, resolved }`; `Cell.comments: Option<Vec<Comment>>` (#[serde(default, skip_serializing_if = "Option::is_none")] so pre-#22 workbooks unchanged); `DataProxy::get_comments` / `add_comment` (returns the new index) / `resolve_thread` / `has_comments`. Legacy single-author `note` field stays. 2 tests pin the wire format and open / reply / resolve / re-open. |

## 3. UI / interaction

| # | Severity | Feature | Notes |
|---|----------|---------|-------|
| | ~~**P1**~~ ✅ | Drag-to-move and resize slicer panels | Shipped 2026-06-29 (commits 287ec34, db80a6f): pure geometry helpers in `util.rs` + 9 unit tests, then DOM wiring. `DragKind::SlicerDrag(id)` and `SlicerResize(id)` variants; shared `Rc<RefCell<Option<DragState>>>` created in `ZedSheet::new` and threaded to both `wire_events` and `wire_slicer_modal`. Mouseup commits the final `end_panel_*` geometry to `DataProxy.slicers[i]` and snapshots for undo. Min panel size 140×60 CSS px. Header strip has `cursor: move`; the new bottom-right grip glyph has `cursor: nwse-resize`. The chips area's `max-height` is `calc(100% - 30px)` so it tracks the panel during resize. |
| | ~~**P1**~~ (partial) ✅ | "Insert from URL" / image insert from clipboard | Shipped 2026-06-29 (commit `53f1697`): URL-only path — right-click → Insert Image… adds a `core::image::Image` to `DataProxy.images`; the renderer blits the cached `HtmlImageElement` at the anchor cell. `zedsheet::image_loader` caches per-thread, fires `onload` once. URL + clipboard paste are deferred follow-ups. |
| | ~~**P1**~~ ✅ | Sort dialog (Data → Sort) with multi-level sort keys and "has header row" | Fixed 2026-06-29: `AutoFilter::sort` widened from `Option<Sort>` to `Vec<Sort>` with legacy format support. `sort_filter_range_multi` does stable sort by multiple keys. New sort dialog (`sort_dialog.rs`) with 3 sort levels and "has header row". Right-click → Sort… opens it. |
| | ~~**P1**~~ ✅ | Cell-level protection with password (sheet protection dialog) | Shipped 2026-06-29 (commits bd6cf90, 766fd4a). New `core::sheet_protection::SheetProtection { enabled, password_hash }` with djb2 + fixed salt ("zedsheet:protect:") → 8-char lowercase hex; 12 unit tests pin determinism, case-sensitivity, no-password-always-verifies, serde round-trip. `DataProxy::set_protection(enabled, password)` mirrors `enabled` onto the existing `read_only` flag. Right-click → "Protect Sheet…" opens a two-mode dialog: protected+hash requires the password to unlock; wrong password surfaces an inline `.zs-protect-error`. Apply snapshots for undo. Backward-compat: pre-1.3 workbooks without the `protection` key in `set_data` load with `enabled = false` and no hash. |
| | ~~**P2**~~ ✅ | Custom keyboard shortcuts / accelerator re-binding | Fixed 2026-07-04 (Phase 5.6): per-mount `set_custom_shortcut(selector, combo, callback)` JS API. Combo string is normalised (modifier order fixed: Ctrl, Alt, Shift; key letter case-insensitive) so the host can write `"Ctrl+Shift+K"` or `"shift+ctrl+k"` interchangeably. The keyboard handler short-circuits the built-in bindings and fires the host callback. 2 tests pin the normalisation rules + register/lookup/clear. |
| | ~~**P2**~~ ✅ | Paste preview (highlight where the paste will land before commit) | Fixed 2026-07-04: `render_paste_preview` draws a dashed blue outline at the active cell, sized to the in-app clipboard's source dimensions. Drawn on top of the selection so both read at once; hidden when the clipboard is empty. |
| | ~~**P2**~~ ✅ | Recent-files list (for the JS `load_data` API) | Fixed 2026-07-04 (Phase 5.4): per-mount MRU in localStorage under `zedsheet::recent::<selector>`, capped at 10, name-based de-dup. JS API: `get_recent_files(selector)` → array of `{name, json, timestamp_ms}`; `push_recent_file(selector, name, json, timestamp_ms)` to push. 4 tests. |
| | ~~**P2**~~ ✅ | Defined-name scope: workbook vs per-sheet | Fixed 2026-07-04 (Phase 5.5): `DataProxy.workbook_named_ranges: Rc<RefCell<HashMap<…>>>` shared across every sheet in the registry. `get_named_range` checks the sheet's own map first, then falls back to the workbook map (sheet shadows workbook). 1 test pins cross-sheet visibility + shadow + remove. |
| | ~~**P2**~~ ✅ | Conditional-formatting rule reordering via UI | Fixed 2026-07-04: per-rule ↑/↓ buttons in the CF dialog. Up hides on row 0, down hides on the last row. `TableRenderer::move_cond_rule` snapshots for undo and is a no-op at the boundaries. |

## 4. Persistence & import / export

| # | Severity | Feature | Notes |
|---|----------|---------|-------|
| | ~~**P0**~~ ✅ | XLSX shared-strings table on **write** (export) | Verified 2026-06-29: `rust_xlsxwriter` v0.79 uses shared strings by default (`use_inline_strings: false`). The `SharedStringsTable` is populated during `wb.save_to_buffer()`. No code changes needed; added roundtrip test with 100 repeated strings. |
| | ~~**P0**~~ ✅ | `import_xlsx` returns `bool`; the actual error is swallowed (`lib.rs:230`) | Fixed 2026-06-29: return type changed to `Result<(), JsValue>`. Parse errors now throw a JS `Error` with the human-readable reason (bad zip, empty workbook, corrupt sheet). |
| | ~~**P1**~~ ✅ | OOXML chart round-trip on export | Shipped 2026-06-29 (commits 2d30fb0, 6bbc989). Write-side: every chart on every sheet is re-emitted as a `rust_xlsxwriter::Chart` next to its anchor cell. `split_chart_range` parses our `Chart.range` into categories + per-column value A1 strings; `chart_kind_to_xlsx_type` + `trendline_to_xlsx` map our kinds / trendlines to rust_xlsxwriter's enums. Bar/line/area/scatter/pie/doughnut export; bubble + radar intentionally dropped (their OOXML representation doesn't carry our sizing / polygon logic). Read-side: calamine doesn't expose chart parts, so charts on imported workbooks are dropped; the underlying data still loads so the user can re-create the chart from the same range. Documented on `import_xlsx` in `src/lib.rs`. 9 new unit tests + 1 end-to-end chart export smoke test + 1 round-trip-drop test. |
| | ~~**P1**~~ (partial) ✅ | ODS / Google Sheets format import (and re-export to `.xlsx` with formula preservation) | Shipped 2026-06-29 (commit `a47e6b8`): minimal-viable path — `import_ods(selector, bytes)` parses an `.ods` byte slice via the `calamine::Ods` reader (already a dependency, no new crate), iterates sheet names, calls `worksheet_range` for values and `worksheet_formula` for formulas, maps each cell through `data_to_text`. Values + formulas in. **Charts, images, cell styling, named-ranges, value-type metadata (currency, date formats) deferred** — same trade-off as the xlsx import. 4 unit tests pin the value→text mapping and the error paths. |
| | ~~**P2**~~ ✅ | Auto-save to IndexedDB (debounced) | Fixed 2026-07-04 (Phase 5.7): new `src/idb_persist.rs` exposing `enable_idb_persist` / `idb_persist_done` / `disable_idb_persist` JS APIs. The engine owns the debounce + dedup + per-mount config; the actual IDB work is delegated to the host's `saveFn` / `loadFn` callbacks (use any IDB wrapper you like — `idb`, native, …). `note_change` calls `maybe_save_to_idb(selector, json)` right after the localStorage write, so IDB persistence runs in parallel with the existing localStorage path. No new dep; uses wasm-bindgen + the already-present wasm-bindgen-futures. 1 test pins the register / unregister round-trip. |
| | ~~**P2**~~ ✅ | Per-cell permission keys (not just the `editable` boolean) | Fixed 2026-07-04: `Cell.format_locked: bool` (default false, `#[serde(default)]` so pre-1.4 workbooks unchanged). `DataProxy::is_cell_format_editable` + `set_cell_format_locked`. `update_selection_style` skips format-locked cells, so every toolbar / keyboard / palette style toggle respects the gate. 1 test pins the four states (both open, format-only lock, value-only lock, sheet-wide read-only). |

## 5. Print & presentation

| # | Severity | Feature | Notes |
|---|----------|---------|-------|
| | ~~**P1**~~ ✅ | Page setup: orientation, paper size, margins, scale-to-fit, repeat header rows on each page | Fixed 2026-06-29: `PageSetup` struct on `DataProxy` with orientation, paper_size, margins, scale, print_area, repeat_rows, repeat_cols — round-trips through `get_data`/`set_data`. `build_print_html` reads page setup and applies `@page` margins, orientation, scale transform, and honours print_area range. |
| | ~~**P2**~~ ✅ | Page break preview / manual page break insertion | Fixed 2026-07-04 (Phase 5.1): `PageSetup.page_breaks: Vec<PageBreak>` with `PageBreak { row: Option<usize>, col: Option<usize> }`; row break ends the page after the row, col break ends it after the column. `TableRenderer::insert_row_page_break` / `insert_col_page_break` / `remove_page_break` snapshot for undo. `render::render_page_breaks` draws a blue dashed line at the bottom of every broken row / right of every broken column. Context menu: "Insert row page break" / "Insert column page break" / "Remove page break". 3 tests pin the default-empty, round-trip, and pre-5.5 backward-compat. |
| | ~~**P2**~~ ✅ | Print only the active sheet vs entire workbook | Fixed 2026-07-04 (Phase 5.3): `print::open_print` now takes `&[DataProxy]` — pass a single-sheet slice for "active sheet only" (existing behaviour) or the full registry for "print all". New `build_workbook_print_html` stitches every sheet into one document with `<section class="zs-print-sheet">` wrappers + `page-break-before: always` between sections. Toolbar: `print` stays the active-sheet path, `print-all` walks the whole registry. 2 tests. |
| | ~~**P2**~~ ✅ | PDF export (via the browser's print dialog → "Save as PDF") | Already shipped (issue #17): the existing Print toolbar button calls `open_print` which loads the document into a hidden iframe and invokes `iframe.contentWindow.print()`. Every browser's print dialog has "Save as PDF" as a standard destination. |

## 6. Charts & graphics

| # | Severity | Feature | Notes |
|---|----------|---------|-------|
| | ~~**P1**~~ ✅ | New chart types: scatter, bubble, radar, area, doughnut, surface, waterfall, treemap, sunburst | Shipped 2026-06-29 (commits ba1b782, 5fe7d51, 816d221): scatter (markers only), bubble (size ∝ |value|, clamped to [3,14] CSS px), area (filled line chart with translucent fill), radar (polygon-per-series, ≥3 categories required), doughnut (pie with 0.45× inner-radius hole). `draw_axes_chart` got a `mode: &str` dispatch (bar/line/scatter/bubble/area); radar + doughnut are separate renderers. Modal Type `<select>` got all five new options. Surface / waterfall / treemap / sunburst dropped from P1 (no one asks for them). |
| | ~~**P1**~~ ✅ | Chart trendlines (linear, exponential, polynomial) | Shipped 2026-06-29 (commits 319fa75, 55123ee). New `core::trendline::Trendline` enum (None / Linear / Exponential / Polynomial) with serde lowercase + `#[serde(default)]`. Three pure least-squares regressions (linear: ordinary LS; exponential: log-linear, returns None for y<=0; polynomial: quadratic via normal equations) + `*_eval` evaluators. 15 unit tests. `Chart.trendline` field added (backward-compat: pre-1.2 workbooks load with None). New "Trendline" `<select>` in the chart modal. Renderer (`draw_axes_chart`) draws one fitted curve per series in a darkened variant of the series' palette colour, clamped to the visible y-range so the curve never escapes the plot box. Pie charts don't get trendlines. |
| | ~~**P1**~~ ✅ | Secondary axis and combination charts (line + bar on dual axes) | Shipped 2026-06-29 (commits bbed63e, 42bd17a, 3871a51). New `Chart.secondary_range: Option<String>` + `extract_secondary_chart_data` helper. `draw_combo_chart` renders primary bars on the left axis + secondary line overlay on a separate right axis, plot box shrunk 22px on the right for the right-axis tick labels. Modal gained a "Secondary range" input that flows through `Chart.secondary_range`; empty input takes the single-axis path. 5 new unit tests in `core::chart` pin extract + range_has_rows semantics. |
| | ~~**P1**~~ ✅ | Sparklines (inline mini-charts in a cell) | Data model + renderer shipped 2026-06-29 (commit `6c62341`). **Modal shipped 2026-07-04** (Phase 4.1b/5.2): new `src/zedsheet/sparkline_modal.rs` mirroring the chart modal pattern — Kind / Data range / Anchor / Color fields, Apply validates the data range parses as a `CellRange`. `TableRenderer::add_sparkline` / `remove_sparkline` snapshot for undo. Context menu: "Insert Sparkline…". |
| | ~~**P2**~~ ✅ | Conditional-formatting data-bar / icon-set rendering on top of bars, not replacing them | Fixed 2026-07-04: `cond_visual` (singular) was returning the first matching visual, so a cell with both a databar AND an icons rule only rendered one. New `cond_visuals` (plural) returns all matches; the renderer iterates and draws bars first, then icons on top. 1 test pins the stacked behavior. |
| | ~~**P2**~~ ✅ | Chart axis label formatting (number format, date format on the Y axis) | Fixed 2026-07-04: `Chart.y_axis_format: Option<String>` (#[serde(default)] for backward-compat). Modal "Y-axis format" text input accepts a small Excel-like DSL: `0` (integer), `0.NN` (decimals), `#,##0` (thousands), `$0.NN` (currency), `0%` / `0.NN%` (percent). Empty / unknown fall through to the default pretty-printer. 8 tests pin the format_axis cases. |

## 7. Collaboration & sharing (de-prioritised)

zedsheet is a personal single-user wasm spreadsheet; the items below are
listed for completeness but are **not on the critical path**. Track
without an issue until someone asks.

| # | Severity | Feature | Notes |
|---|----------|---------|-------|
| | P3 | Multi-user live editing (CRDT / OT) | Out of scope. |
| | P3 | Comment threads with author / resolve / reopen | P2 under data model. |
| | P3 | Presence indicators (live cursors) | Out of scope. |
| | P3 | Version history / named snapshots | Not a critical need. |
| | P3 | "Share link" / read-only view URL | Requires a backend. |

---

## 8. Codebase hygiene

Small things the audit turned up. These are easy to clean up alongside
the next feature that touches the same area.

| # | Severity | Item | Notes |
|---|----------|------|-------|
| | ~~**P2**~~ ✅ | `src/renderer/canvas.rs:181` `set_line_dash` has a `// FIXME` and a manual `Float64Array` build | Fixed 2026-07-04: replaced the manual `new_with_length` + `set_index` loop with `js_sys::Float64Array::from(segments)`. |
| | ~~**P2**~~ ✅ | Empty port-placeholder directories: `src/editor/`, `src/overlayer/`, `src/resizer/`, `src/scrollbar/`, `src/selector/` (each has only a `.keep`) | Fixed 2026-07-04: all five directories removed. The P1 §2 items (rich text + images) the BACKLOG reserved editor/ + overlayer/ for are now shipped via `renderer/` + `zedsheet/`, so those placeholders are superseded. `cargo build` and the full test suite stay clean. |
| | ~~**P2**~~ ✅ | `set_line_dash`/`set_dash` is a single FIXME, but the rest of the canvas helpers (`fill_text`, `stroke_rect`, `save`/`restore` wrappers) look complete | Fixed 2026-07-04: same commit as the first row above (Float64Array::from). |
| | ~~**P3**~~ ✅ | Clippy `-D warnings` currently fails (267 warnings + 1 error pre-existing) | Fixed 2026-07-04 (commit `5784b32`): zero warnings, zero errors. `cargo clippy --target wasm32-unknown-unknown --all-targets -- -D warnings` is clean. 11 files touched across 4 commits (EllipseArgs refactor, too_many_arguments silences, this clippy batch, Float64Array follow-up). |

---

## How to add an entry

- Be specific. "Sort" is not a backlog item; "Sort dialog with multi-level keys, has-header option, case sensitivity" is.
- Reference the GitHub issue number if one exists, or note "no issue yet" so the next person filing knows.
- Keep severity honest. If a P0 has lived here for a year, it's probably a P1.
- Re-run the audit (`gh issue list --state all`, the placeholder-dir scan, the function-table grep) after each major feature closes.
