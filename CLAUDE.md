# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

A web spreadsheet written in Rust and compiled to WebAssembly — a from-scratch port of [x-spreadsheet](https://github.com/myliang/x-spreadsheet). The grid is drawn on a single `<canvas>`; the surrounding chrome (toolbar, formula bar, sheet tabs, modals) is plain DOM, all driven from Rust via `wasm-bindgen` / `web-sys`. There is no JavaScript framework and no JS source beyond the build scripts. The crate is `cdylib`-only and is published to npm as `zedsheet`.

## Commands

```sh
# Compile check (the real target — fastest feedback for type errors)
cargo build --target wasm32-unknown-unknown

# Unit tests — run NATIVELY on the host, not under wasm
cargo test
cargo test format::            # one module
cargo test xlsx_roundtrip      # tests matching a substring

# Run the standalone demo with live reload
trunk serve

# Production bundle into dist/
trunk build --release

# Build the publishable npm package into pkg/ (needs wasm-pack)
node scripts/build-npm.mjs
```

One-time setup: `rustup target add wasm32-unknown-unknown`, `cargo install trunk`, and `cargo install wasm-pack` (only for the npm build).

When verifying behavior in a browser, prefer building once and serving the static `dist/` (`cd dist && python3 -m http.server 8099`) over `trunk serve` — Trunk's auto-reload re-initializes the WASM module and can re-run the mount mid-session.

## Architecture

Data flows in one direction: **`DataProxy` (model) → `TableRenderer` (canvas) → DOM chrome re-renders after each interaction.**

- **`src/lib.rs`** — wasm-bindgen entry point and the entire public JS API: `mount`, `get_data`, `load_data`, `on_change`, `export_csv`/`import_csv`, `export_xlsx`/`import_xlsx`, `setSheetReadOnly`/`isSheetReadOnly`. Mounted workbooks live in a `thread_local!` `MOUNTS` map keyed by CSS selector; each entry holds closures (`GetDataFn`, `LoadDataFn`, …) captured from the `ZedSheet` before it is `std::mem::forget`-ed. `start()` auto-mounts demo data only if a `#zedsheet` element exists (the Trunk demo's `index.html` has one; host apps don't).

- **`src/core/data_proxy.rs`** (~5k lines) — single source of truth for one sheet: cells, styles, merges, freeze, selection, validation, conditional formats, outline groups, charts, **and the formula evaluator** (`eval_expr`). `src/formula/parser.rs` is only the tokenizer; evaluation lives here because it needs cell access. Cross-sheet references (`Sheet2!A1`) resolve through `SheetsRegistry` (`Rc<RefCell<Vec<DataProxy>>>`), which every `DataProxy` holds a `Weak` link to; the circular-reference guard (`Visited`) is keyed by `(sheet_name, row, col)` and threaded through cross-sheet hops.

- **`src/renderer/`** — canvas drawing. `table_renderer.rs` owns view state, hit-testing, selection, and mutation entry points; `render.rs` does the actual body/header/selection/scrollbar drawing; `viewport.rs` maps scroll position to visible cell ranges.

- **`src/zedsheet/`** — the DOM chrome, split into submodules (`toolbar`, `formula_bar`, `context_menu`, `events`, the modals, `print`, `find_replace`, …). `mod.rs` owns the `ZedSheet` shell, `::new` orchestration, and the shared type aliases (`SharedRenderer = Rc<RefCell<TableRenderer>>`, `Sheets`, `SyncFn`, …); every submodule does `use super::*` and `mod.rs` re-exports each submodule's entry points, so they share types and call each other through the parent.

- **`src/component/`** — lower-level DOM helpers: `element.rs` (`h()` element builder), toolbar item definitions, `Options`.

- **`src/persist.rs`** — per-mount localStorage persistence and the `on_change` callback, with change de-duplication. `finish_mount` in `lib.rs` deliberately restores saved data *before* arming persistence so the initial render can't clobber a saved workbook.

- **`src/core/workbook.rs`** — workbook JSON (de)serialization (x-spreadsheet format, the wire format of `get_data`/`load_data`). `core/csv.rs` and `core/xlsx.rs` (calamine for read, rust_xlsxwriter for write) convert to/from external formats.

Shared mutable state is `Rc<RefCell<…>>` throughout; DOM event handlers are `Closure`s that capture clones of these handles and call the shared `SyncFn` to refresh toolbar/formula-bar state after mutations.

## Conventions

- `#![allow(dead_code)]` is set crate-wide on purpose: modules were ported from x-spreadsheet ahead of being wired in. The empty `.keep` directories under `src/` (`editor/`, `overlayer/`, `resizer/`, …) are placeholders from the port. Don't "clean up" unwired code unless you've confirmed it is superseded.
- Comments frequently cite issue numbers (`issue #20`); keep doing this when implementing tracked features.
- Tests are inline `#[cfg(test)] mod tests` blocks in the module they test (281 tests across `core/`, `renderer/`, `formula/`, `zedsheet/`). They run on the native target, so test pure logic — nothing that touches `web-sys` at runtime.
- `src/index.css` is the live stylesheet (the `.less` file is vestigial); the CSS class prefix is `CSS_PREFIX = "zedsheet"` from `src/config.rs`. The stylesheet must keep referencing the toolbar sprite as exactly `url('/asset/sprite.svg')` — `scripts/build-npm.mjs` matches that literal string to inline it, and errors if it changes.
- The workbook JSON format must stay round-trippable: new per-cell/per-sheet state needs `serde` support in `data_proxy.rs`/`workbook.rs` (use `#[serde(default)]` for new optional fields) so old saved workbooks still load.

## Known gaps (from README)

`#REF!` for deleted-cell references (left stale), absolute references (`$A$1`) in some paths, autofilter UI, locale/i18n.
