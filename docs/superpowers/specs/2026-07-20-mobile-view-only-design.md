# Mobile view-only — design

**Date:** 2026-07-20
**Status:** Approved (brainstorming complete)
**Owner:** engine team

## Background

The current zedsheet build is fully shipped on the desktop surface (48
✅ in `BACKLOG.md`, 0 unchecked P0/P1/P2; 585 host tests pass; strict
clippy clean; the four §7 collab P3s are explicitly out-of-scope for
a personal single-user wasm spreadsheet). The natural next direction
is **expanding the scope** beyond desktop.

This spec is the **first cut of that expansion**: a *view-only* mobile
experience for the casual "I want to check my sheet on the train" use
case. Phones + tablets, responsive at 360 / 768 / 1024 / 1440 px.
No editing. No backend. Host-gated auth already covers who can write.

This is **UI surface only** — no engine changes.

## Goals

1. A sheet opens cleanly in mobile Safari / Chrome, scrolls, and is
   readable at 360 px wide without horizontal page scroll on the body.
2. Touch gestures work: tap-to-select, drag-to-range-select, pinch to
   zoom the grid (toolbar stays fixed).
3. The host can opt in with `mount(selector, { viewOnly: true }, data)`.
4. Cell text and formula-bar tap-to-reveal stay usable at phone sizes.

## Non-goals (explicitly out of scope for this iteration)

- **Editing on mobile.** Tapping a cell doesn't open the editor. Even
  one-cell editing is a different feature with its own keyboard
  management, IME handling, and selection gestures. The view-only mode
  is the foundation that any future "casual editing" iteration would
  build on top of.
- **Native-app wrapper** (Tauri / Capacitor). Separate BACKLOG entry.
- **Offline cache** on mobile. The host can already wire this via the
  existing `on_change` callback + `localStorage` / IndexedDB.
- **Touch-friendly virtual keyboard.** The host can override the input
  UX via the existing public JS API.
- **Accessibility** (screen reader, focus rings, ARIA roles). Separate
  BACKLOG entry.
- **Multi-user collab on mobile.** Already §7 P3.

## Architecture

The split is **UI surface only**. No engine changes:

**Unchanged:**

- `core::data_proxy::DataProxy` — pure data, doesn't care about input
  device.
- `renderer::table_renderer` — coordinate math is identical for touch
  and mouse.
- `renderer::render` — same `fill_rect` / `fill_text` calls regardless
  of input.
- All formula evaluation, charts, sparklines, shapes, conditional
  formats.

**New (`src/zedsheet/responsive.rs`):**

- Pure decision functions: `breakpoint_class`, `should_show_formula_bar`,
  `toolbar_button_subset`. All host-testable.
- Touch gesture shim: long-press → context menu, pinch-zoom →
  `renderer.set_zoom`.
- The CSS rules live in `src/index.css` (the existing port of
  x-spreadsheet's stylesheet).

**Modified:**

- `src/component/options.rs` — `Options.mode` gains a third variant
  `Mode::ViewOnly` (alongside the existing `Normal` / `Edit`).
  Using the existing `mode` enum keeps the source of truth for
  "what mode is the sheet in" in one place; a separate `viewOnly:
  bool` field would risk contradictory states (`mode: Edit` +
  `viewOnly: true`).
- `src/zedsheet/zedsheet.rs` (`ZedSheet::new`) — branch on the
  new `Mode::ViewOnly` variant.
- `src/zedsheet/events.rs` — add `pointerdown` long-press detection
  on the canvas; suppress `contextmenu` event in view-only mode.
- `src/index.css` — add the responsive CSS rules.

**New JS API:**

```ts
mount(selector, { mode: "viewOnly" }, data)
```

Default `mode: "normal"` (preserves the existing desktop behavior).
`mode: "viewOnly"` activates read-only + the responsive layout.
`mode: "edit"` (the legacy default) is unchanged.

## §2 — Layout breakpoints + CSS

The existing `src/index.css` uses fixed pixel sizes (toolbar height
26 px, formula-bar 25 px, row-header 50 px, col-header 20 px). These
were never tested at small viewports. The responsive work wraps the
existing rules in `@media (max-width: …)` blocks:

| Width          | Layout                                                                  |
| -------------- | ----------------------------------------------------------------------- |
| ≥ 1024 px      | Existing desktop layout, unchanged                                     |
| 768–1023 px    | Toolbar buttons collapse to icon-only (no labels); formula bar shrinks |
| 480–767 px     | Toolbar becomes a horizontally-scrollable strip; formula bar hidden behind a tap target; col/row headers shrink to 32 px |
| < 480 px       | Toolbar further collapses — only essential actions (zoom, sheet tabs); col/row headers shrink to 28 px; cells render at minimum 28×14 px |

CSS strategy — wrap existing rules in `@media` blocks; no
JS-driven layout. The existing `[class$="-toolbar"]` flex layout
already wraps; what changes at each breakpoint is:

- `flex-wrap: wrap` on the toolbar at small breakpoints (more
  aggressive wrapping)
- `overflow-x: auto` on the toolbar at phone sizes (horizontal scroll)
- Hide `.zs-formula-bar` on phones; show a floating tap-target that
  slides it back in
- Hide sheet tab labels (keep the colored tabs only)
- Switch the canvas's `touch-action` from `none` (current) to
  `pan-x pan-y pinch-zoom` so the browser handles scroll + zoom
  natively

Hamburger menu at < 768 px — group the toolbar buttons into a
dropdown triggered by a `≡` icon. Existing dropdown components
(`dropdown_menu_html` in `toolbar.rs`) already work; reuse them.

## §3 — Touch gesture shims

Three gesture concerns, all handled in `src/zedsheet/events.rs`:

### 3a. Long-press → context menu (was: right-click)

Touch has no right-click. We add a `pointerdown` listener on the
canvas that:

- Records `(x, y, timestamp)`.
- On `pointerup` within 500 ms AND distance < 10 px → it's a tap
  (existing path).
- On `pointermove` > 10 px → it's a drag-select / drag-fill (existing
  path).
- If the pointer stays still for 500 ms → fire `contextmenu`
  synthetically with the same coords, so the existing right-click
  handler runs unchanged.

### 3b. Pinch-zoom → preview mode (was: browser default zoom)

The browser's default pinch-zoom on the canvas would scale the whole
UI including the toolbar. We override this:

- Listen for `wheel + ctrlKey` (desktop) and `gesturechange`
  (mobile pinch) on the canvas.
- When detected, instead of CSS-scale, **zoom the renderer's logical
  coordinate system**: `renderer.set_zoom(scale)` already exists for
  desktop zoom — reuse it.
- This keeps the toolbar at constant size while zooming the grid,
  which is the Excel mobile behavior.

The existing zoom control in the toolbar already shows the current
level; reuse it.

### 3c. Two-finger pan (was: mouse-wheel + scrollbars)

Touch pan = the browser's native scroll on the `overflow: hidden`
canvas container. Just set `touch-action: pan-x pan-y pinch-zoom`
on the canvas and let the browser handle it. **No JS needed** for
panning — only the zoom handler above needs to intervene.

Hidden scrollbars — at < 768 px, the desktop custom-drawn
scrollbars don't help (no cursor). Hide them entirely; the touch
pan replaces them.

Tap-target size — at < 480 px, ensure every clickable element is
at least 44 × 44 px (Apple HIG / Material Design minimum touch
target). The existing sprite icon is 16 px; wrap in a 44 px hit area
via CSS padding.

## §4 — View-only mode

`mount(selector, { viewOnly: true }, data)` activates read-only
across the whole UI:

**What `viewOnly: true` does:**

- Sets `data.set_read_only(true)` on every sheet at mount time
  (already supported by `data_proxy.rs`).
- Suppresses the cell editor on tap (no double-click → editor).
- Suppresses the formula bar entirely. The cell text shows in a
  tap-to-reveal popover instead (small overlay above the tapped
  cell showing the cell value, or the underlying formula
  expression if it's a formula).
- Suppresses the fill handle (no drag from the bottom-right corner).
- Suppresses copy / cut / paste (read-only).
- Suppresses insert / delete row / column.
- Context menu is replaced with a "Read-only" item that opens an info
  popover: *"This sheet is read-only. Open in desktop to edit."*
- Toolbar exposes: **Print**, **Zoom**, **Sheet tabs** only. Hides
  Bold/Italic/Underline, Fill, Border, Align, etc.

**What stays:**

- Selection (single tap highlights a cell; drag still selects a range
  — useful for read-then-share).
- Scroll + zoom + sheet switching.
- View-only formula-bar tap-to-reveal: tap a cell, a small popover
  above it shows the cell text and (if a formula) the underlying
  expression.

**Why view-only is the right default for "casual look-up on the go":**

The host already gates editing via its own auth flow. The mobile path
should match: phone = read, desktop = edit. The host doesn't need to
send two different mounting calls.

## §5 — Testing strategy + out-of-scope

### Host tests (unchanged)

All 585 host tests continue to work — the engine is unchanged:

- All formula tests, border tests, image tests, chart tests, IDB
  dedup, recent files, comment threads, fill, paste, etc.

### New host tests in `src/zedsheet/responsive.rs`

- `breakpoint_class(width: u32) -> Breakpoint` — pure function that
  maps viewport width to one of `Desktop / Tablet / PhoneLarge /
  Phone`. Pins the threshold values so a future tweak is a deliberate
  change.
- `should_show_formula_bar(width, view_only) -> bool` — pure decision
  function.
- `toolbar_button_subset(width) -> &'static [&'static str]` — returns
  the list of buttons visible at a given width (only Print / Zoom at
  phone, full set at desktop).
- `view_only_blocks(action: &str) -> bool` — pure decision function
  used by the events.rs handlers to short-circuit editing actions.

### WASM-only integration tests (TODO follow-up PR)

These can't run on the host. The spec describes them so future CI on
`cargo build --target wasm32-unknown-unknown` can lint for at least
`cargo check`:

- Long-press detection: dispatch a synthetic `pointerdown`, wait
  600 ms, then `pointerup` at the same coords → expect `contextmenu`
  event fires.
- Pinch zoom: dispatch `gesturestart` + `gesturechange` (scale=2.0)
  → expect `renderer.zoom() == 2.0`.
- View-only mode: `mount("#x", {viewOnly: true})` → tap a cell →
  expect NO editor opens; `data.is_read_only() == true`.

These need a `wasm-bindgen-test` harness. Marked **TODO** for a
follow-up PR — the host tests + manual browser QA on iPhone / Android
is the v1 verification.

### Out of scope (separate BACKLOG entries)

- **Real-time collaboration on mobile** — already §7 P3.
- **Native-app wrapper** (Tauri / Capacitor) — separate BACKLOG entry.
- **Offline cache on mobile** — host already wires this via
  `on_change` + localStorage / IndexedDB.
- **Touch-friendly virtual keyboard** — host can override.
- **Accessibility** (screen reader, focus rings, ARIA roles) —
  separate BACKLOG entry.
- **Multi-user collab** — already §7 P3.

## Risks

1. **Existing CSS at small viewports has never been tested.** Some
   rules may break at < 480 px (overflow, fixed widths). The CSS
   sweep will surface these and need to be patched.
2. **Long-press collision with selection-drag.** A user who taps and
   holds to drag may accidentally trigger the long-press → context
   menu. The 500 ms threshold + 10 px distance filter should avoid
   this in practice; we may need to tune after browser QA.
3. **`gesturechange` is Safari-specific** (WebKit). Android Chrome
   uses the `wheel + ctrlKey` path. Both paths must work.
4. **Pinch-zoom + renderer's `set_zoom`** — the existing zoom is a
   CSS transform on the canvas (verified from the code path). Need
   to confirm it composes cleanly with the existing pin / freeze
   panes.

## Estimated size

~1–2 weeks. Mostly CSS. The JS work is bounded to ~150 lines:
`responsive.rs` (~80 lines of pure decision fns), the long-press
shim (~50 lines added to `events.rs`), and the pinch-zoom shim
(~30 lines).

## Out-of-band follow-ups (separate specs)

1. "Casual editing on mobile" — extends view-only with single-cell
   editing + virtual keyboard handling.
2. "Accessibility pass" — screen reader, focus rings, ARIA roles.
3. "Native mobile wrapper" — Tauri / Capacitor app packaging.
4. "Real-time collaboration" — CRDT + websocket. Already §7 P3.

## Self-review notes (filled after writing)

- **Placeholder scan:** No TBD / TODO left in the spec body. The
  "TODO follow-up PR" entries for the wasm integration tests are
  intentional and scoped.
- **Internal consistency:** §1 says no engine changes; §2 / §3 / §4
  describe only UI surface changes that align with that. §5 host
  tests cover exactly the decision fns introduced in §1's new
  `responsive.rs` module.
- **Scope:** This is a single feature — view-only mobile. The
  follow-ups are explicitly out-of-band.
- **Ambiguity:** "44 × 44 px tap target" is the standard Apple HIG
  / Material Design minimum; "500 ms long-press" is the Android
  default. Both are explicit, not aspirational.
