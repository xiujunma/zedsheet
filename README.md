# Zedsheet

A web spreadsheet written in **Rust** and compiled to **WebAssembly**. It is a
from-scratch port of [x-spreadsheet](https://github.com/myliang/x-spreadsheet):
the grid is drawn on a single `<canvas>` and the surrounding chrome (toolbar,
formula bar, sheet tabs, menus) is plain DOM, all driven from Rust via
`wasm-bindgen` / `web-sys`.

## Features

**Grid & data**
- Canvas-rendered grid with row/column headers, gridlines, and frozen panes
- Per-cell styling: background, text color, bold/italic/underline/strike,
  font family & size, horizontal & vertical alignment, text wrap
- Merged cells (merge-aware selection, navigation, and rendering)
- Number / currency (¥ $ €) / percent formatting with thousands separators
- Formula engine: `SUM, AVERAGE, MAX, MIN, COUNT, PRODUCT, ABS, ROUND, IF`,
  cell refs and ranges (`A1`, `B2:C4`), with a circular-reference guard
- Formula references auto-adjust when rows/columns are inserted or deleted

**Interaction**
- Click to select, drag to range-select (works in every direction), keyboard
  navigation, mouse-wheel scroll
- In-cell editor (double-click or Enter); commit on Enter/Tab, cancel on Esc
- Row/column resize by dragging header borders; draggable scrollbar thumbs
- Clipboard: copy / cut / paste, plus `Ctrl/Cmd + C/X/V`, `Ctrl/Cmd + B/I/U`,
  and `Delete` to clear

**Chrome**
- Icon toolbar (sprite-based) with live active-state that reflects the
  selected cell — format / font / font-size dropdowns, text & fill color
  pickers, bold/italic/underline/strike, merge, align, wrap, freeze
- Excel-style **formula bar**: name box (navigate to a cell or `A1:B3` range),
  cancel/confirm, an `fx` function picker, and a formula input bound to the
  active cell
- Right-click **context menu**: copy/cut/paste, insert/delete row & column,
  clear contents
- **Multi-sheet** tabs: add, switch, double-click to rename, right-click to delete

## Prerequisites

- A recent stable [Rust toolchain](https://rustup.rs/)
- The WebAssembly target:
  ```sh
  rustup target add wasm32-unknown-unknown
  ```
- [Trunk](https://trunkrs.dev/):
  ```sh
  cargo install trunk
  ```

## Running

Serve with live reload:

```sh
trunk serve
```

Or build the static bundle into `dist/` and serve it with any static server:

```sh
trunk build            # production: trunk build --release
cd dist && python3 -m http.server 8099
```

Then open the printed URL. The app mounts into the `#zedsheet` element in
[`index.html`](./index.html).

### Using it in your own app (React, etc.)

Build an importable package with `wasm-pack` and call the exported `mount`:

```sh
wasm-pack build --target web --out-dir pkg --out-name zedsheet
```

```js
import init, { mount } from "zedsheet";
await init();
mount("#my-grid");   // mounts an interactive spreadsheet into that element
```

See **[docs/REACT.md](./docs/REACT.md)** for the full React walkthrough
(component, styles, sprite asset, bundler notes).

> **Tip:** when verifying behavior, prefer building once and serving the static
> `dist/` over `trunk serve` — Trunk's auto-reload re-initializes the WASM
> module and can re-run the mount during a session.

## Project layout

```
src/
├── lib.rs              # wasm-bindgen entry point + sample data
├── zedsheet.rs         # ZedSheet: builds the DOM shell and wires all events
├── config.rs           # CSS prefix and constants
├── index.css           # ported x-spreadsheet styles
├── core/               # data model
│   ├── data_proxy.rs   # single source of truth (cells, styles, merges,
│   │                   #   freeze, selection, formula evaluation)
│   ├── cell.rs, row.rs, col.rs, cols.rs, cell_range.rs, merges.rs
│   ├── format.rs       # number/currency/percent formatting (unit-tested)
│   ├── state.rs, validation.rs, auto_filter.rs, helper.rs
├── formula/            # tokenizer + functions (evaluator lives in data_proxy)
├── renderer/           # canvas drawing
│   ├── table_renderer.rs  # state, hit-testing, selection, mutations
│   ├── render.rs          # body / headers / selection / scrollbar drawing
│   ├── viewport.rs, area.rs, range.rs, canvas.rs, border.rs,
│   ├── cell_render.rs, alphabets.rs
└── component/          # DOM components (toolbar, element helpers, options, …)
```

The renderer consumes `core::data_proxy::DataProxy` directly; the DOM chrome in
`zedsheet.rs` holds the renderer in an `Rc<RefCell<…>>` and re-renders the
canvas after each interaction.

## Development

```sh
# Compile to wasm (fast feedback)
cargo build --target wasm32-unknown-unknown

# Run unit tests (native)
cargo test
```

## Status & known limitations

Most of x-spreadsheet's interactive surface is implemented. Unit tests are
ported for `alphabet`, `cell_range`, `format`, the formula evaluator, and
`helper` (`cargo test`). Not yet done:

- `#REF!` for references to deleted cells (currently left stale)
- Absolute references (`$A$1`) and deeper nested-function arg parsing
- Data validation / autofilter UI, print, and locale/i18n

## Credits

Port of [x-spreadsheet](https://github.com/myliang/x-spreadsheet) by myliang
(MIT). Icon sprite assets originate from that project.
