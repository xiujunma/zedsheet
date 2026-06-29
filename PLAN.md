# PLAN — Backlog P1 features

> Concrete plan to ship every P1 item in `BACKLOG.md`. Each item lands
> as **multiple commits within the feature** (engine / UI / tests) so
> review and bisect stay easy.
>
> **Re-baseline:** 2026-06-23 from the audit in `BACKLOG.md`. The 10
> P1s marked ✅ in BACKLOG are already done; this plan covers the 12
> that remain.

## The 12 P1 items, grouped by phase

### Phase 1 — Quick wins (≈ 1 week)

| # | Feature | Scope | Files touched | Commits |
|---|---------|-------|---------------|---------|
| 1 | **Drag-to-move & resize slicer panels** | Half-done: `Slicer` carries `x/y/w/h`; only the DOM drag/resize handler is missing. Follow the column/row resize pattern. Undo (snapshot on drop) too. | `src/zedsheet/slicer_modal.rs`, `src/zedsheet/events.rs`, `src/zedsheet/mod.rs`, `src/renderer/table_renderer.rs` (cursor for panel hover) | (a) data-model: undoable move/resize via `snapshot` + new helper, (b) UI: mousedown/move/up on panels, (c) tests |
| 2 | **Chart trendlines** | Linear, exponential, polynomial. Per-series opt-in. Drawn over the existing chart canvas after the series is drawn. | `src/core/chart.rs`, `src/renderer/chart_render.rs`, `src/zedsheet/chart_modal.rs` | (a) data model: `trendline: Option<Trendline>` field + serde, (b) renderer: linear regression + draw, poly/exp variants, (c) modal: UI toggle, (d) tests |
| 3 | **Sheet protection dialog (with password)** | `editable` flag is per-cell (issue #24); add a UI dialog that flips the flag based on a sheet-level "Protected" toggle. Password gates the *toggle*, not the per-cell flag itself. | `src/zedsheet/protection_modal.rs` (new), `src/zedsheet/mod.rs`, `src/core/data_proxy.rs` (sheet-level password hash) | (a) engine: `SheetProtection { enabled, password_hash }` field, (b) UI: dialog + toolbar toggle, (c) tests |

### Phase 2 — Chart family (≈ 2-3 weeks)

| # | Feature | Scope | Files touched | Commits |
|---|---------|-------|---------------|---------|
| 4 | **New chart types: scatter, area, doughnut, bubble, radar** | Five renderers + five modal `<option>`s. Scatter/bubble share `draw_axes_chart` extension. Area is a filled-line variant. Doughnut is a pie variant with a hole. Radar is a new polygon renderer. | `src/renderer/chart_render.rs`, `src/zedsheet/chart_modal.rs`, `src/core/chart.rs` | (a) scatter + bubble, (b) area + radar, (c) doughnut, (d) tests |
| 5 | **Secondary axis & combination charts** | Per-series `axis: Primary | Secondary`. Renderer draws two Y axes. Combination = mix of bar + line on the same chart. | `src/core/chart.rs`, `src/renderer/chart_render.rs`, `src/zedsheet/chart_modal.rs` | (a) per-series axis field, (b) dual-axis render, (c) combination modal, (d) tests |
| 6 | **OOXML chart round-trip on export** | Use rust_xlsxwriter's chart API to write embedded chart parts. Read existing chart XML on import via calamine's chart support (if available) or skip and document the loss. | `Cargo.toml`, `src/core/xlsx.rs` | (a) write-side: per-kind chart export, (b) read-side: import where possible + warn on loss, (c) tests |

### Phase 3 — Formula engine depth (≈ 3-4 weeks)

| # | Feature | Scope | Files touched | Commits |
|---|---------|-------|---------------|---------|
| 7 | **Array literals `{1,2,3;4,5,6}`** | Tokenizer: a new `Token::ArrayLiteral(Vec<Vec<Token>>)`. Parser: emit `Value::Array` directly. Spill path already exists. Constants in expression position: `={1;2;3}+10` → `{11;12;13}` (broadcast already works). | `src/formula/parser.rs`, `src/core/data_proxy.rs` | (a) tokenizer: array-literal parsing, (b) parser: emit Value::Array, (c) MMULT + integration tests |
| 8 | **LET / LAMBDA / MAP / REDUCE / BYROW / BYCOL / MAKEARRAY** | LET: name-binding in `eval_expr` (a `HashMap<String, Value>` frame on the call stack). LAMBDA: define a `Lambda(Value)` variant + delayed evaluation. MAP/REDUCE/BYROW/BYCOL/MAKEARRAY: lambda-accepting functions. | `src/formula/parser.rs`, `src/core/data_proxy.rs` | (a) LET (small, foundational), (b) LAMBDA + name-binding scope, (c) MAP / REDUCE, (d) BYROW / BYCOL / MAKEARRAY, (e) tests |

### Phase 4 — Cell & graphics depth (≈ 6-8 weeks)

| # | Feature | Scope | Files touched | Commits |
|---|---------|-------|---------------|---------|
| 9 | **Image insert from URL + clipboard** | New `Image` struct on `DataProxy` (anchor cell + src + size). Renderer: image cache, lazy `fetch` on demand, draw on top of the body in render pass. Modal: paste a URL or paste clipboard image. | `src/core/image.rs` (new), `src/core/data_proxy.rs`, `src/renderer/render.rs`, `src/zedsheet/image_modal.rs` (new), `src/zedsheet/mod.rs` | (a) data model + serde round-trip, (b) renderer: fetch + draw, (c) modal: URL input + clipboard paste, (d) resize handle, (e) tests |
| 10 | **Sparklines** | Per-cell inline mini-chart. Three kinds: line, column, win/loss. New `Sparkline` struct (`kind`, `range`, optional `color`). Renderer: tiny chart drawn into the cell rect. UI: insert via right-click menu. | `src/core/sparkline.rs` (new), `src/core/data_proxy.rs`, `src/renderer/render.rs`, `src/zedsheet/sparkline_modal.rs` (new) | (a) data model + range extract, (b) renderer: line/column/w-l, (c) modal, (d) tests |
| 11 | **Rich text / inline formatting** | `Cell.runs: Option<Vec<Run>>` where `Run = { text: String, style: Option<usize> }`. `#[serde(default)]` for backward-compat. Renderer: per-run `set_font`/`fill_text` loop. Modal: per-run editor (select text → toggle bold/italic/color). xlsx round-trip: rust_xlsxwriter rich-string format. | `src/core/cell.rs`, `src/core/data_proxy.rs`, `src/renderer/render.rs`, `src/zedsheet/rich_text_modal.rs` (new), `src/core/xlsx.rs` | (a) data model + serde compat, (b) renderer: per-run loop, (c) modal: per-run formatting, (d) xlsx export, (e) xlsx import, (f) tests |
| 12 | **ODS / Google Sheets import** | ODS via new crate (likely `rsheet` or `simple_ods`). Sheets via the Sheets API or a CSV-style intermediate. Round-trip: keep formulas where possible, document loss. Lower priority — only if explicitly requested. | `Cargo.toml`, `src/core/ods.rs` (new), `src/lib.rs` (new wasm-bindgen export), `tests/` | (a) crate selection + ODS read, (b) sheet hydration, (c) tests |

## Dependency graph

```
Phase 1 ─┬─ (independent of all later phases)
         │
Phase 2 ─┼─ (independent of Phase 3)
         │
Phase 3 ─┤  ┌── 9 (image insert) ─┐
         ├──┤                      ├── 11 (rich text) is the
         │  └── 10 (sparklines) ──┤    natural follow-on; the
         │                        │    per-run drawing infra
         │                        │    is the same
         │
Phase 4 ─┘
```

## Estimated total work

| Phase | Items | Estimate |
|-------|-------|---------|
| 1     | 3     | 1 week  |
| 2     | 3     | 2-3 weeks |
| 3     | 2     | 3-4 weeks |
| 4     | 4     | 6-8 weeks |
| **Total** | **12** | **~3-4 months** |

## Working agreement

- One feature at a time. Each phase ends with a clean test run + WASM build + a brief summary in chat.
- Stop at the end of each phase for review before moving on.
- If a feature grows past its estimate, surface it before committing to the next.
- BACKLOG.md is updated at the end of each phase to ✅ the items shipped.

## Open questions

- Phase 4 #12 (ODS/Sheets): low ROI. Defer or drop?
- Phase 2 #4: drop surface/treemap/sunburst? They render well but no one asks for them.
- Phase 3 #8: MAP/REDUCE/BYROW/BYCOL/MAKEARRAY only matter once LAMBDA exists. Bundle them with LAMBDA, or punt?