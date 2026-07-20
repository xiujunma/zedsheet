# Mobile view-only implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Mobile view-only experience for zedsheet — phones + tablets in a browser, read-only casual look-up. CSS-only responsive layout + small JS gesture shims; no engine changes.

**Architecture:** Single new file `src/zedsheet/responsive.rs` for pure decision fns + DOM event handlers. Add `Mode::ViewOnly` variant to the existing `Options.mode` enum. Add @media-query CSS rules to `src/index.css`. Reuse existing `set_zoom` and `set_read_only` from `table_renderer.rs` / `data_proxy.rs`.

**Tech Stack:** Rust + wasm-bindgen + existing flex-based DOM layout. No new deps.

## Global Constraints

- **Engine untouched.** `DataProxy`, `TableRenderer`, the formula engine, charts, sparklines, shapes, conditional formats — all unchanged. Mobile view-only is UI surface only.
- **Strict clippy clean** — `cargo clippy --target wasm32-unknown-unknown --all-targets -- -D warnings` must pass after every task.
- **All 585 existing tests must still pass** — no engine changes means they should; pure additive changes only.
- **`#[allow(dead_code)]` is set crate-wide** (per `CLAUDE.md`) — but new fns should still be exercised by tests; don't add unused fns.
- **Commit cadence:** one commit per task. Use the existing `feat:` / `chore:` / `docs:` / `test:` / `refactor:` prefixes.
- **No new deps.** All implementation uses existing crates (`wasm-bindgen`, `web-sys`, `gloo-utils`, `wasm-bindgen-futures`).

## File Structure

**Create:**
- `src/zedsheet/responsive.rs` — pure decision fns (`breakpoint_class`, `should_show_formula_bar`, `toolbar_button_subset`, `view_only_blocks`) + `wire_long_press` + `wire_pinch_zoom` JS handlers. ~150 lines.
- `docs/superpowers/specs/2026-07-20-mobile-view-only-design.md` — already exists from the spec phase; read it for context.

**Modify:**
- `src/component/options.rs` — add `Mode::ViewOnly` variant. ~3 lines.
- `src/zedsheet/zedsheet.rs` — branch on `Mode::ViewOnly` at the bottom of `ZedSheet::new`. ~15 lines.
- `src/zedsheet/events.rs` — pass an optional `viewOnly` flag into the keydown handler chain. ~10 lines.
- `src/index.css` — append @media query blocks + a `[data-readonly="true"]` selector + tap-target padding. ~80 lines.

**Test:** all tests live inside `#[cfg(test)] mod tests` in `src/zedsheet/responsive.rs` — pure host tests, no wasm needed.

---

### Task 1: Add `Mode::ViewOnly` variant + propagate to `ZedSheet::new`

**Files:**
- Modify: `src/component/options.rs:8-14` (add `Mode::ViewOnly` variant + doc comment)
- Modify: `src/zedsheet/zedsheet.rs` (branch on `Mode::ViewOnly` at the bottom of `ZedSheet::new` — set `data.set_read_only(true)` on every sheet when ViewOnly)
- Test: existing tests (no new tests — the variant is a one-liner; the set_read_only behavior is already tested)

**Interfaces:**
- Consumes: nothing (existing `Options` field)
- Produces: `Options.mode == Mode::ViewOnly` triggers `data.set_read_only(true)` on every `DataProxy` in the registry at mount time.

- [ ] **Step 1: Add `Mode::ViewOnly` to `src/component/options.rs`**

```rust
#[derive(Debug, Clone)]
pub enum Mode {
    Normal,
    Edit,
    /// Read-only mount for casual look-up on mobile. Disables
    /// the cell editor, the formula bar, copy/cut/paste, and
    /// the fill handle. Toolbar collapses to read-only actions
    /// (Print, Zoom, Sheet tabs). Phase 7 (mobile view-only).
    ViewOnly,
}
```

- [ ] **Step 2: Run the existing tests to confirm the variant compiles**

Run: `cargo test --lib 2>&1 | tail -3`
Expected: `test result: ok. 585 passed; 0 failed`

- [ ] **Step 3: Branch on `Mode::ViewOnly` at the bottom of `ZedSheet::new`**

Open `src/zedsheet/zedsheet.rs`, find `ZedSheet::new`. Right after the existing `let active: ActiveSheet = Rc::new(RefCell::new(0));` line, add:

```rust
// Phase 7: view-only mode flips every sheet to read-only at
// mount time. The rest of the UI gating (editor, paste, fill,
// context menu) is handled by downstream checks against
// `data.is_read_only()` + the `view_only_blocks` decision fn.
if matches!(options.mode, crate::component::options::Mode::ViewOnly) {
    for d in sheets.borrow_mut().iter_mut() {
        d.set_read_only(true);
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test --lib 2>&1 | tail -3 && cargo clippy --target wasm32-unknown-unknown --all-targets -- -D warnings 2>&1 | tail -3`
Expected: 585 passed, clippy clean

- [ ] **Step 5: Commit**

```bash
git add src/component/options.rs src/zedsheet/zedsheet.rs
git commit -m "feat(mobile): add Mode::ViewOnly variant and propagate to mount"
```

---

### Task 2: Pure decision fns in `src/zedsheet/responsive.rs` (TDD)

**Files:**
- Create: `src/zedsheet/responsive.rs` — the new module
- Test: `#[cfg(test)] mod tests` at the bottom of the same file

**Interfaces:**
- Consumes: nothing (pure functions of viewport width + viewOnly flag)
- Produces:
  - `pub enum Breakpoint { Desktop, Tablet, PhoneLarge, Phone }`
  - `pub fn breakpoint_class(width: u32) -> Breakpoint`
  - `pub fn should_show_formula_bar(width: u32, view_only: bool) -> bool`
  - `pub fn toolbar_button_subset(width: u32) -> &'static [&'static str]`
  - `pub fn view_only_blocks(action: &str) -> bool`

- [ ] **Step 1: Create the module with the enum + `breakpoint_class` skeleton**

Create `src/zedsheet/responsive.rs`:

```rust
//! Mobile view-only responsive helpers (Phase 7).
//!
//! Pure decision functions — viewport-width → layout class, view-only
//! → formula bar visibility, etc. Host-testable: no DOM / JS needed.

/// Which layout bucket the current viewport falls into.
/// Buckets are decided by `breakpoint_class`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Breakpoint {
    Desktop,
    Tablet,
    PhoneLarge,
    Phone,
}

/// Classify the viewport width into a layout bucket. The
/// thresholds match the spec's breakpoint table (1024 / 768 / 480).
pub fn breakpoint_class(width: u32) -> Breakpoint {
    if width >= 1024 {
        Breakpoint::Desktop
    } else if width >= 768 {
        Breakpoint::Tablet
    } else if width >= 480 {
        Breakpoint::PhoneLarge
    } else {
        Breakpoint::Phone
    }
}
```

- [ ] **Step 2: Write the failing test for `breakpoint_class`**

Append to the file (placeholder for now; we'll add the test module in step 5):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn breakpoint_class_thresholds() {
        // Pin every threshold + the two endpoints so a future
        // tweak is a deliberate change.
        assert_eq!(breakpoint_class(1440), Breakpoint::Desktop);
        assert_eq!(breakpoint_class(1024), Breakpoint::Desktop); // boundary
        assert_eq!(breakpoint_class(1023), Breakpoint::Tablet);
        assert_eq!(breakpoint_class(768), Breakpoint::Tablet);  // boundary
        assert_eq!(breakpoint_class(767), Breakpoint::PhoneLarge);
        assert_eq!(breakpoint_class(480), Breakpoint::PhoneLarge); // boundary
        assert_eq!(breakpoint_class(479), Breakpoint::Phone);
        assert_eq!(breakpoint_class(360), Breakpoint::Phone);
    }
}
```

- [ ] **Step 3: Run test to confirm it passes**

Run: `cargo test --lib breakpoint 2>&1 | tail -5`
Expected: PASS (the function already exists)

- [ ] **Step 4: Add `should_show_formula_bar` + its test**

Append to `responsive.rs`:

```rust
/// True when the formula bar should render. The bar is hidden
/// at < 768 px (saving vertical space on phones + tablets), and
/// always hidden in view-only mode (the cell editor is suppressed,
/// so the bar would be dead UI).
pub fn should_show_formula_bar(width: u32, view_only: bool) -> bool {
    !view_only && width >= 768
}
```

And add to the test module:

```rust
    #[test]
    fn formula_bar_hidden_on_phones_and_in_view_only() {
        assert!(should_show_formula_bar(1440, false));
        assert!(should_show_formula_bar(768, false));
        assert!(!should_show_formula_bar(767, false));
        assert!(!should_show_formula_bar(360, false));
        // View-only always hides the bar, even on desktop.
        assert!(!should_show_formula_bar(1440, true));
        assert!(!should_show_formula_bar(768, true));
    }
```

- [ ] **Step 5: Add `toolbar_button_subset` + its test**

Append:

```rust
/// The toolbar buttons visible at the given width. Desktop shows
/// the full set; tablet strips the text labels (icon-only via
/// the existing dropdown component); phone collapses further to
/// only essential actions. Phase 7 ships the data layer here;
/// the actual CSS hides the rest at each breakpoint.
pub fn toolbar_button_subset(width: u32) -> &'static [&'static str] {
    // The literal action ids mirror what's in
    // `src/zedsheet/context_menu.rs` / `src/zedsheet/toolbar.rs`.
    // Keep this list in sync if new toolbar actions are added.
    const DESKTOP: &[&str] = &[
        "undo", "redo", "print", "font-bold", "font-italic",
        "underline", "strike", "color", "bgcolor", "merge",
        "borders", "halign", "valign", "textwrap", "freeze",
        "autofilter", "formula",
    ];
    const TABLET: &[&str] = &[
        "undo", "redo", "print", "font-bold", "font-italic",
        "underline", "freeze", "autofilter", "formula",
    ];
    const PHONE_LARGE: &[&str] = &[
        "undo", "redo", "print", "freeze", "autofilter", "formula",
    ];
    const PHONE: &[&str] = &[
        "print", "autofilter", "formula",
    ];
    match breakpoint_class(width) {
        Breakpoint::Desktop => DESKTOP,
        Breakpoint::Tablet => TABLET,
        Breakpoint::PhoneLarge => PHONE_LARGE,
        Breakpoint::Phone => PHONE,
    }
}
```

And add to the test module:

```rust
    #[test]
    fn toolbar_subset_shrinks_with_viewport() {
        // Desktop has the full set; phone has only the essentials.
        let desktop = toolbar_button_subset(1440);
        let tablet = toolbar_button_subset(800);
        let phone_large = toolbar_button_subset(600);
        let phone = toolbar_button_subset(360);
        assert!(desktop.len() > tablet.len());
        assert!(tablet.len() > phone_large.len());
        assert!(phone_large.len() > phone.len());
        // Phone shows print + sheet switcher + formula picker — the
        // survival kit, nothing else.
        assert!(phone.contains(&"print"));
        assert!(phone.contains(&"autofilter"));
        assert!(phone.contains(&"formula"));
    }
```

- [ ] **Step 6: Add `view_only_blocks` + its test**

Append:

```rust
/// True when the given toolbar / context-menu action should be
/// suppressed in view-only mode. Used by the events.rs handlers
/// to short-circuit before the existing mutation paths.
pub fn view_only_blocks(action: &str) -> bool {
    // Single source of truth for "what's allowed in view-only".
    // The non-listed actions (selection, scroll, zoom, sheet tabs)
    // remain enabled.
    matches!(
        action,
        "edit"            // double-click → cell editor
        | "copy"
        | "cut"
        | "paste"
        | "paste-values"
        | "paste-formulas"
        | "paste-formats"
        | "paste-transpose"
        | "paste-link"
        | "insert-row"
        | "insert-col"
        | "delete-row"
        | "delete-col"
        | "insert-cells-down"
        | "insert-cells-right"
        | "delete-cells-up"
        | "delete-cells-left"
        | "clear"
        | "font-bold" | "font-italic" | "underline" | "strike"
        | "color" | "bgcolor"
        | "merge" | "borders"
        | "halign" | "valign" | "textwrap" | "rotate-0" | "rotate-45"
        | "rotate-90" | "rotate--45" | "shrink-toggle"
        | "indent-inc" | "indent-dec"
        | "lock-unlock"
        | "validation" | "condfmt" | "chart" | "image" | "sparkline"
        | "pivot" | "slicer" | "protect" | "refresh-pivot"
        | "group-rows" | "ungroup-rows" | "group-cols" | "ungroup-cols"
        | "subtotal" | "sort-range"
        | "format-table" | "format-as-rich" | "format-as-plain"
        | "table-totals" | "table-to-range"
        | "page-break-row" | "page-break-col" | "page-break-remove"
        | "shape"
    )
}
```

And add to the test module:

```rust
    #[test]
    fn view_only_blocks_editing_actions_only() {
        // Blocked: every action that mutates data.
        for a in [
            "edit", "copy", "cut", "paste",
            "insert-row", "delete-col",
            "font-bold", "color", "merge",
            "condfmt", "chart", "image", "shape",
        ] {
            assert!(view_only_blocks(a), "{a} should be blocked in view-only");
        }
        // Not blocked: navigation + read-only actions.
        for a in ["print", "autofilter", "formula"] {
            assert!(!view_only_blocks(a), "{a} should remain enabled in view-only");
        }
        // Unknown action: not blocked (default = allow).
        assert!(!view_only_blocks("does-not-exist"));
    }
```

- [ ] **Step 7: Run all tests + clippy**

Run:
```bash
cargo test --lib responsive 2>&1 | tail -5
cargo test --lib 2>&1 | tail -2
cargo clippy --target wasm32-unknown-unknown --all-targets -- -D warnings 2>&1 | tail -3
```
Expected: 4 new responsive tests pass, 589 total pass (585 + 4), clippy clean

- [ ] **Step 8: Commit**

```bash
git add src/zedsheet/responsive.rs
git commit -m "feat(mobile): pure decision fns for view-only responsive layout

Host-testable helpers: breakpoint_class, should_show_formula_bar,
toolbar_button_subset, view_only_blocks. Each pins a threshold or
set membership so a future tweak is a deliberate change. 4 new
tests, 589 total."
```

---

### Task 3: Responsive CSS rules in `src/index.css`

**Files:**
- Modify: `src/index.css` (append @media query blocks + tap-target helpers)

**Interfaces:**
- Consumes: existing CSS selectors (`.zedsheet-toolbar`, `.zs-formula-bar`, etc.)
- Produces: a `[data-readonly="true"]` selector that hides every editing affordance; `@media (max-width: …)` rules that collapse the toolbar, hide the formula bar, shrink row/col headers at the spec's 4 breakpoints.

- [ ] **Step 1: Read the current `src/index.css` to confirm the class names**

Run: `grep -E "zedsheet-toolbar|zs-formula-bar|zedsheet-table|zedsheet-context" src/index.css | head -10`
Expected: hits on the existing selectors (confirms the names match the spec's assumptions).

- [ ] **Step 2: Append the responsive rules**

Append to `src/index.css`:

```css
/* =====================================================================
 * Mobile / touch / view-only (Phase 7)
 * =====================================================================
 * Breakpoint table (mirrors breakpoint_class in zedsheet/responsive.rs):
 *   >= 1024 px  desktop
 *    768-1023   tablet   (toolbar icon-only)
 *    480- 767   phone-l  (toolbar scrollable, formula hidden)
 *     <  480    phone    (toolbar further collapsed)
 * ================================================================== */

/* Tablet: drop the toolbar labels, keep the icons. */
@media (max-width: 1023px) {
  .zedsheet-toolbar-btn .zedsheet-icon-img + span { display: none; }
}

/* Phone-large: scrollable toolbar, formula bar hidden, smaller
 * row/col headers, tap-target padding for every button. */
@media (max-width: 767px) {
  .zedsheet-toolbar-btn {
    /* Material Design minimum 44 x 44 tap target. */
    min-width: 44px;
    min-height: 44px;
    padding: 8px;
  }
  .zedsheet-toolbar-btns {
    flex-wrap: nowrap;
    overflow-x: auto;
    overflow-y: hidden;
  }
  .zedsheet-formula-bar { display: none; }
  .zedsheet-row-header,
  .zedsheet-col-header { width: 32px; height: 32px; }
  .zedsheet-table { touch-action: pan-x pan-y pinch-zoom; }
}

/* Phone: collapse toolbar further — only essential actions
 * (Print, AutoFilter, Sheet picker). Hide sheet tab labels
 * (keep the colored tab dots). */
@media (max-width: 479px) {
  .zedsheet-toolbar-btn[data-action="undo"],
  .zedsheet-toolbar-btn[data-action="redo"],
  .zedsheet-toolbar-btn[data-action="font-bold"],
  .zedsheet-toolbar-btn[data-action="font-italic"],
  .zedsheet-toolbar-btn[data-action="underline"],
  .zedsheet-toolbar-btn[data-action="strike"],
  .zedsheet-toolbar-btn[data-action="borders"],
  .zedsheet-toolbar-btn[data-action="halign"],
  .zedsheet-toolbar-btn[data-action="valign"],
  .zedsheet-toolbar-btn[data-action="rotate-0"],
  .zedsheet-toolbar-btn[data-action="rotate-45"],
  .zedsheet-toolbar-btn[data-action="rotate-90"],
  .zedsheet-toolbar-btn[data-action="rotate--45"] { display: none; }
  .zedsheet-row-header,
  .zedsheet-col-header { width: 28px; height: 28px; }
  .zedsheet-sheet-tab-label { display: none; }
}

/* View-only mode (Phase 7): the host sets [data-readonly="true"]
 * on the root container. Every editing affordance is hidden. */
.zedsheet[data-readonly="true"] .zedsheet-toolbar-btn[data-action="undo"],
.zedsheet[data-readonly="true"] .zedsheet-toolbar-btn[data-action="redo"],
.zedsheet[data-readonly="true"] .zedsheet-toolbar-btn[data-action="font-bold"],
.zedsheet[data-readonly="true"] .zedsheet-toolbar-btn[data-action="font-italic"],
.zedsheet[data-readonly="true"] .zedsheet-toolbar-btn[data-action="underline"],
.zedsheet[data-readonly="true"] .zedsheet-toolbar-btn[data-action="strike"],
.zedsheet[data-readonly="true"] .zedsheet-toolbar-btn[data-action="color"],
.zedsheet[data-readonly="true"] .zedsheet-toolbar-btn[data-action="bgcolor"],
.zedsheet[data-readonly="true"] .zedsheet-toolbar-btn[data-action="merge"],
.zedsheet[data-readonly="true"] .zedsheet-toolbar-btn[data-action="borders"],
.zedsheet[data-readonly="true"] .zedsheet-toolbar-btn[data-action="halign"],
.zedsheet[data-readonly="true"] .zedsheet-toolbar-btn[data-action="valign"],
.zedsheet[data-readonly="true"] .zedsheet-toolbar-btn[data-action="textwrap"],
.zedsheet[data-readonly="true"] .zedsheet-toolbar-btn[data-action="rotate-0"],
.zedsheet[data-readonly="true"] .zedsheet-toolbar-btn[data-action="rotate-45"],
.zedsheet[data-readonly="true"] .zedsheet-toolbar-btn[data-action="rotate-90"],
.zedsheet[data-readonly="true"] .zedsheet-toolbar-btn[data-action="rotate--45"],
.zedsheet[data-readonly="true"] .zedsheet-toolbar-btn[data-action="shrink-toggle"],
.zedsheet[data-readonly="true"] .zedsheet-toolbar-btn[data-action="indent-inc"],
.zedsheet[data-readonly="true"] .zedsheet-toolbar-btn[data-action="indent-dec"],
.zedsheet[data-readonly="true"] .zedsheet-toolbar-btn[data-action="freeze"],
.zedsheet[data-readonly="true"] .zedsheet-toolbar-btn[data-action="autofilter"],
.zedsheet[data-readonly="true"] .zedsheet-toolbar-btn[data-action="formula"],
.zedsheet[data-readonly="true"] .zedsheet-toolbar-btn[data-action="paintformat"],
.zedsheet[data-readonly="true"] .zedsheet-toolbar-btn[data-action="clearformat"],
.zedsheet[data-readonly="true"] .zedsheet-toolbar-btn[data-action="print"],
.zedsheet[data-readonly="true"] .zedsheet-toolbar-btn[data-action="borders"],
.zedsheet[data-readonly="true"] .zedsheet-toolbar-btn[data-action="format-table"],
.zedsheet[data-readonly="true"] .zedsheet-toolbar-btn[data-action="sparkline"],
.zedsheet[data-readonly="true"] .zedsheet-toolbar-btn[data-action="shape"],
.zedsheet[data-readonly="true"] .zedsheet-toolbar-btn[data-action="sort-range"],
.zedsheet[data-readonly="true"] .zedsheet-toolbar-btn[data-action="format-as-rich"],
.zedsheet[data-readonly="true"] .zedsheet-toolbar-btn[data-action="format-as-plain"],
.zedsheet[data-readonly="true"] .zedsheet-toolbar-btn[data-action="group-rows"],
.zedsheet[data-readonly="true"] .zedsheet-toolbar-btn[data-action="group-cols"],
.zedsheet[data-readonly="true"] .zedsheet-toolbar-btn[data-action="subtotal"],
.zedsheet[data-readonly="true"] .zedsheet-btn[role="button"]:not([data-action="print"]) { display: none !important; }
.zedsheet[data-readonly="true"] .zedsheet-formula-bar { display: none !important; }
.zedsheet[data-readonly="true"] .zedsheet-fill-handle { display: none !important; }
```

- [ ] **Step 3: Verify the build still compiles + clippy clean**

Run:
```bash
cargo check --target wasm32-unknown-unknown 2>&1 | tail -3
cargo clippy --target wasm32-unknown-unknown --all-targets -- -D warnings 2>&1 | tail -3
```
Expected: clean

- [ ] **Step 4: Commit**

```bash
git add src/index.css
git commit -m "feat(mobile): responsive CSS rules + view-only selector

Adds 4 @media query blocks for the 1024 / 768 / 480 breakpoints
(toolbar icon-only on tablet, scrollable on phone-large, hidden
labels on phone, 44 px tap targets) + a [data-readonly=\"true\"]
selector that hides every editing affordance. The host sets the
attribute on the root container when Mode::ViewOnly is active."
```

---

### Task 4: Wire long-press → context menu

**Files:**
- Modify: `src/zedsheet/events.rs` (add a `wire_long_press` function and call it from `wire_events`)
- Modify: `src/zedsheet/zedsheet.rs` (call `wire_long_press` from `ZedSheet::new`)
- Modify: `src/zedsheet/responsive.rs` (add the `wire_long_press` JS shim + a `#[cfg(target_arch = \"wasm32\")] pub fn` that takes the canvas + mount_selector)

**Interfaces:**
- Consumes: a `web_sys::Element` (the canvas), the mount selector (so the click doesn't fire view_only's own logic).
- Produces: a `pointerdown` listener that synthesizes a `contextmenu` event after 500 ms of no-movement / no-up.

- [ ] **Step 1: Add `wire_long_press` to `responsive.rs`**

Append to `responsive.rs`:

```rust
#[cfg(target_arch = "wasm32")]
pub mod wasm {
    use super::*;
    use wasm_bindgen::closure::Closure;
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::spawn_local;
    use web_sys::PointerEvent;

    const LONG_PRESS_MS: i32 = 500;
    const LONG_PRESS_SLOP_PX: f64 = 10.0;

    /// Wire a long-press → contextmenu gesture on `canvas_el`.
    /// Touch has no right-click; we synthesise a `contextmenu`
    /// event after 500 ms of no-movement so the existing
    /// right-click handler runs unchanged.
    pub fn wire_long_press(canvas_el: &web_sys::Element) {
        let canvas_for_down = canvas_el.clone();
        let canvas_for_timer = canvas_el.clone();
        let down_state: Rc<RefCell<Option<(f64, f64, i32, i32)>>> =
            Rc::new(RefCell::new(None));

        let down_cb = Closure::<dyn FnMut(PointerEvent)>::new(move |ev: PointerEvent| {
            let (x, y) = (ev.offset_x() as f64, ev.offset_y() as f64);
            *down_state.borrow_mut() = Some((x, y, ev.pointer_id(), 0));
        });
        canvas_for_down.add_event_listener_with_callback(
            "pointerdown",
            down_cb.as_ref().unchecked_ref(),
        );
        down_cb.forget();

        // Move handler: if the pointer moves > LONG_PRESS_SLOP_PX,
        // cancel the pending long-press (it's a drag, not a tap).
        let move_state = down_state.clone();
        let canvas_for_move = canvas_el.clone();
        let move_cb = Closure::<dyn FnMut(PointerEvent)>::new(move |ev: PointerEvent| {
            if let Some((x0, y0, _, _)) = *move_state.borrow() {
                let (x, y) = (ev.offset_x() as f64, ev.offset_y() as f64);
                if (x - x0).abs() > LONG_PRESS_SLOP_PX || (y - y0).abs() > LONG_PRESS_SLOP_PX {
                    *move_state.borrow_mut() = None;
                }
            }
        });
        canvas_for_move.add_event_listener_with_callback(
            "pointermove",
            move_cb.as_ref().unchecked_ref(),
        );
        move_cb.forget();

        // Up handler: clear the pending state.
        let up_state = down_state.clone();
        let canvas_for_up = canvas_el.clone();
        let up_cb = Closure::<dyn FnMut(PointerEvent)>::new(move |_ev: PointerEvent| {
            *up_state.borrow_mut() = None;
        });
        canvas_for_up.add_event_listener_with_callback(
            "pointerup",
            up_cb.as_ref().unchecked_ref(),
        );
        up_cb.forget();

        // Timer: fire `contextmenu` synthetically after 500 ms if
        // the pointer is still in the down state. We poll the
        // state every 50 ms — cheap, and avoids juggling
        // `setTimeout` handles in JS.
        let timer_state = down_state.clone();
        let canvas_for_timer_inner = canvas_for_timer.clone();
        let timer_cb = Closure::<dyn FnMut()>::new(move || {
            let st = timer_state.borrow().clone();
            if let Some((x, y, pid, _)) = st {
                // Synthesise contextmenu so the existing right-
                // click handler runs. We dispatch on the canvas.
                let init = web_sys::MouseEventInit::new();
                init.set_client_x(x as i32);
                init.set_client_y(y as i32);
                init.set_button(2); // right
                if let Ok(ev) = web_sys::MouseEvent::new_with_mouse_event_init_dict(
                    "contextmenu",
                    &init,
                ) {
                    let _ = canvas_for_timer_inner.dispatch_event(&ev);
                }
                // Reset state so we don't double-fire.
                *timer_state.borrow_mut() = None;
                let _ = pid; // currently unused; reserved for future PointerEvent dispatch
            }
        });
        let _ = web_sys::window().map(|w| {
            spawn_local(async move {
                loop {
                    let p = js_sys::Promise::resolve(&JsValue::NULL);
                    let _ = wasm_bindgen_futures::JsFuture::from(p).await;
                    timer_cb.as_ref().unchecked_ref::<js_sys::Function>()
                        .call0(&JsValue::NULL)
                        .ok();
                }
            });
            w.set_interval_with_callback_and_timeout_and_arguments_0(
                timer_cb.as_ref().unchecked_ref(),
                LONG_PRESS_MS,
            )
        });
        timer_cb.forget();
    }
}
```

- [ ] **Step 2: Build to verify the wasm shim compiles**

Run: `cargo check --target wasm32-unknown-unknown 2>&1 | tail -10`
Expected: clean

- [ ] **Step 3: Wire the call from `ZedSheet::new`**

In `src/zedsheet/zedsheet.rs`, find the line that calls `wire_events(...)`. Right after it, add:

```rust
// Phase 7: long-press gesture (mobile only — desktop right-clicks
// already trigger contextmenu natively). Hostile to read-only
// mode? No — view-only suppresses the resulting context-menu
// items in `wire_context_menu` via `view_only_blocks`.
#[cfg(target_arch = "wasm32")]
responsive::wasm::wire_long_press(canvas_el.as_ref());
```

And at the top of `zedsheet.rs`, add:

```rust
use crate::zedsheet::responsive;
```

(if not already imported).

- [ ] **Step 4: Run tests + clippy**

Run:
```bash
cargo test --lib 2>&1 | tail -3
cargo clippy --target wasm32-unknown-unknown --all-targets -- -D warnings 2>&1 | tail -3
```
Expected: 589 passed, clippy clean

- [ ] **Step 5: Commit**

```bash
git add src/zedsheet/responsive.rs src/zedsheet/zedsheet.rs
git commit -m "feat(mobile): wire long-press → contextmenu gesture

500 ms no-movement timeout synthesises a contextmenu event so
the existing right-click handler runs on touch devices. Movement
> 10 px cancels (it's a drag, not a tap)."
```

---

### Task 5: Wire pinch-zoom → `renderer.set_zoom`

**Files:**
- Modify: `src/zedsheet/responsive.rs` (add `wire_pinch_zoom` to the `wasm` module)
- Modify: `src/zedsheet/zedsheet.rs` (call `wire_pinch_zoom` from `ZedSheet::new`)

**Interfaces:**
- Consumes: the canvas element + a `SharedRenderer` (already threaded into `ZedSheet::new`).
- Produces: a `gesturestart` / `gesturechange` listener that calls `renderer.set_zoom(scale)`. Desktop Ctrl+wheel does the same.

- [ ] **Step 1: Add `wire_pinch_zoom` to `responsive.rs`**

Append inside the `pub mod wasm` block:

```rust
    /// Wire pinch-zoom → `renderer.set_zoom`. Both the
    /// `gesturechange` event (WebKit / iOS Safari) and the
    /// `wheel + ctrlKey` event (desktop) route through the
    /// same zoom handler so the desktop Ctrl-wheel zoom and
    /// the mobile pinch zoom share a single scale model.
    pub fn wire_pinch_zoom(
        canvas_el: &web_sys::Element,
        renderer: crate::renderer::table_renderer::SharedRenderer,
    ) {
        let canvas_el_gesture = canvas_el.clone();
        let renderer_gesture = renderer.clone();
        let gesture_cb = Closure::<dyn FnMut(JsValue)>::new(move |ev: JsValue| {
            if let Ok(gesture) = ev.dyn_into::<web_sys::Event>() {
                let scale = js_sys::Reflect::get(&gesture, &JsValue::from_str("scale"))
                    .ok()
                    .and_then(|v| v.as_f64())
                    .unwrap_or(1.0);
                if scale > 0.0 {
                    let mut r = renderer_gesture.borrow_mut();
                    r.set_zoom(scale);
                    let _ = r.render();
                }
            }
        });
        let _ = canvas_el_gesture.add_event_listener_with_callback(
            "gesturechange",
            gesture_cb.as_ref().unchecked_ref(),
        );
        gesture_cb.forget();

        let canvas_el_wheel = canvas_el.clone();
        let renderer_wheel = renderer.clone();
        let wheel_cb = Closure::<dyn FnMut(web_sys::WheelEvent)>::new(move |ev: web_sys::WheelEvent| {
            if !ev.ctrl_key() {
                return;
            }
            let current = renderer_wheel.borrow().zoom();
            let scale = if ev.delta_y() < 0.0 { 1.1 } else { 1.0 / 1.1 };
            let next = (current * scale).clamp(0.1, 4.0);
            let mut r = renderer_wheel.borrow_mut();
            r.set_zoom(next);
            let _ = r.render();
        });
        let _ = canvas_el_wheel.add_event_listener_with_callback(
            "wheel",
            wheel_cb.as_ref().unchecked_ref(),
        );
        wheel_cb.forget();
    }
```

- [ ] **Step 2: Wire the call from `ZedSheet::new`**

In `src/zedsheet/zedsheet.rs`, right after the `wire_long_press` call from Task 4, add:

```rust
#[cfg(target_arch = "wasm32")]
responsive::wasm::wire_pinch_zoom(canvas_el.as_ref(), renderer.clone());
```

- [ ] **Step 3: Run tests + clippy**

Run:
```bash
cargo check --target wasm32-unknown-unknown 2>&1 | tail -5
cargo test --lib 2>&1 | tail -2
cargo clippy --target wasm32-unknown-unknown --all-targets -- -D warnings 2>&1 | tail -3
```
Expected: clean

- [ ] **Step 4: Commit**

```bash
git add src/zedsheet/responsive.rs src/zedsheet/zedsheet.rs
git commit -m "feat(mobile): wire pinch-zoom + desktop Ctrl-wheel to renderer.set_zoom

Routes both gesturechange (WebKit/Safari) and wheel+ctrlKey
(desktop) through the same handler so mobile and desktop zoom
share a single scale model. Clamped to [0.1, 4.0]."
```

---

### Task 6: View-only mode: toolbar filter + `[data-readonly="true"]` attribute + context-menu gating

**Files:**
- Modify: `src/zedsheet/zedsheet.rs` (set `data-readonly="true"` on the root container when `Mode::ViewOnly`)
- Modify: `src/zedsheet/context_menu.rs` (gate every editing action via `view_only_blocks`)

**Interfaces:**
- Consumes: `view_only_blocks` from `responsive.rs`.
- Produces: a context menu that swaps to "Read-only — open in desktop to edit" when view-only, OR each action silently no-ops.

- [ ] **Step 1: Set the data-readonly attribute in `ZedSheet::new`**

In `src/zedsheet/zedsheet.rs`, find where `root` is created and the `if matches!(options.mode, Mode::ViewOnly)` block from Task 1. Right after the `set_read_only(true)` loop, also set the attribute:

```rust
if matches!(options.mode, crate::component::options::Mode::ViewOnly) {
    for d in sheets.borrow_mut().iter_mut() {
        d.set_read_only(true);
    }
    let _ = root.el.as_ref().and_then(|e| e.set_attribute("data-readonly", "true"));
}
```

- [ ] **Step 2: Update `context_menu.rs` to gate via `view_only_blocks`**

In `src/zedsheet/context_menu.rs`, find the `match` / `if cmd ==` block that dispatches toolbar actions. Wrap each `r.some_action()` call with:

```rust
if !crate::zedsheet::responsive::view_only_blocks(cmd) {
    r.some_action();
}
```

(Or, equivalently, add a single early-return at the top of the match:

```rust
if crate::zedsheet::responsive::view_only_blocks(cmd) {
    let _ = menu_for_click
        .unchecked_ref::<web_sys::HtmlElement>()
        .style()
        .set_property("display", "none");
    return;
}
```

Add the early-return AFTER the read-only-aware code, BEFORE the per-cmd match. Use whichever pattern matches the existing context-menu.rs style — the early-return is simpler; do that.

- [ ] **Step 3: Run tests + clippy**

Run:
```bash
cargo test --lib 2>&1 | tail -3
cargo clippy --target wasm32-unknown-unknown --all-targets -- -D warnings 2>&1 | tail -3
```
Expected: 589 passed, clippy clean

- [ ] **Step 4: Commit**

```bash
git add src/zedsheet/zedsheet.rs src/zedsheet/context_menu.rs
git commit -m "feat(mobile): gate context-menu + toolbar actions in view-only

Sets data-readonly=\"true\" on the root container (CSS picks it
up to hide every editing affordance) and short-circuits the
context-menu dispatch via view_only_blocks. Read-only still
allows navigation: tap a cell, drag-select a range, switch
sheets, zoom, print."
```

---

### Task 7: Tap-to-reveal popover (view-only formula-bar replacement)

**Files:**
- Modify: `src/zedsheet/zedsheet.rs` (add a `wire_tap_reveal` JS handler after the events handlers, gated on `Mode::ViewOnly`)
- Modify: `src/zedsheet/responsive.rs` (add a small `wire_tap_reveal` to the `wasm` module)

**Interfaces:**
- Consumes: the canvas + a `SharedRenderer`.
- Produces: a `pointerup` handler that, when in view-only mode, shows a small popover above the tapped cell with the cell's display value (or the underlying formula expression for formula cells).

- [ ] **Step 1: Add `wire_tap_reveal` to `responsive.rs`**

Append inside the `pub mod wasm` block:

```rust
    /// Tap-to-reveal popover. In view-only mode, tapping a
    /// cell shows a small overlay above it with the cell's
    /// display value (or the underlying formula expression for
    /// formula cells). Hides on the next tap or after 5 s.
    pub fn wire_tap_reveal(
        canvas_el: &web_sys::Element,
        renderer: crate::renderer::table_renderer::SharedRenderer,
    ) {
        // Build the popover element once.
        let doc = web_sys::window().and_then(|w| w.document());
        let popover = doc.and_then(|d| d.create_element("div").ok());
        let Some(popover) = popover else { return; };
        let _ = popover.set_attribute("class", "zs-tap-reveal");
        let style = popover.unchecked_ref::<web_sys::HtmlElement>().style();
        let _ = style.set_property("position", "fixed");
        let _ = style.set_property("z-index", "900");
        let _ = style.set_property("background", "#fffbe6");
        let _ = style.set_property("border", "1px solid #d9c97a");
        let _ = style.set_property("padding", "6px 8px");
        let _ = style.set_property("font", "12px Arial, sans-serif");
        let _ = style.set_property("display", "none");
        let _ = style.set_property("pointer-events", "none");
        let _ = style.set_property("max-width", "280px");
        let _ = style.set_property("white-space", "pre-wrap");
        let _ = doc.and_then(|d| d.body()).map(|b| {
            let _ = b.append_child(&popover);
        });

        let popover_for_show = popover.clone();
        let renderer_for_show = renderer.clone();
        let canvas_el_for_show = canvas_el.clone();
        let show_cb = Closure::<dyn FnMut(PointerEvent)>::new(move |ev: PointerEvent| {
            let (x, y) = (ev.offset_x() as f64, ev.offset_y() as f64);
            let hit = renderer_for_show.borrow().cell_at(x, y);
            let Some((_r, _c)) = hit else { return; };
            let text = renderer_for_show.borrow().data.cell_display_value(_r, _c);
            popover_for_show.set_text_content(Some(&text));
            let s = popover_for_show.unchecked_ref::<web_sys::HtmlElement>().style();
            let _ = s.set_property("display", "block");
            let _ = s.set_property("left", &format!("{}px", (ev.client_x() + 8) as i32));
            let _ = s.set_property("top", &format!("{}px", (ev.client_y() + 8) as i32));
            let popover_for_hide = popover_for_show.clone();
            let hide_cb = Closure::<dyn FnMut()>::new(move || {
                let _ = popover_for_hide
                    .unchecked_ref::<web_sys::HtmlElement>()
                    .style()
                    .set_property("display", "none");
            });
            let _ = web_sys::window().map(|w| {
                w.set_timeout_with_callback_and_timeout_and_arguments_0(
                    hide_cb.as_ref().unchecked_ref(),
                    5000,
                )
            });
            hide_cb.forget();
            // Also hide on the next pointerup anywhere.
            let popover_for_hide2 = popover_for_show.clone();
            let next_up = Closure::<dyn FnMut(PointerEvent)>::new(move |_ev: PointerEvent| {
                let _ = popover_for_hide2
                    .unchecked_ref::<web_sys::HtmlElement>()
                    .style()
                    .set_property("display", "none");
            });
            let _ = canvas_el_for_show.add_event_listener_with_callback(
                "pointerup",
                next_up.as_ref().unchecked_ref(),
            );
            next_up.forget();
        });
        let _ = canvas_el.add_event_listener_with_callback(
            "pointerup",
            show_cb.as_ref().unchecked_ref(),
        );
        show_cb.forget();
    }
```

- [ ] **Step 2: Wire the call from `ZedSheet::new`**

In `src/zedsheet/zedsheet.rs`, after the `wire_pinch_zoom` call:

```rust
#[cfg(target_arch = "wasm32")]
if matches!(options.mode, crate::component::options::Mode::ViewOnly) {
    responsive::wasm::wire_tap_reveal(canvas_el.as_ref(), renderer.clone());
}
```

- [ ] **Step 3: Build + clippy + tests**

Run:
```bash
cargo check --target wasm32-unknown-unknown 2>&1 | tail -3
cargo test --lib 2>&1 | tail -2
cargo clippy --target wasm32-unknown-unknown --all-targets -- -D warnings 2>&1 | tail -3
```
Expected: clean

- [ ] **Step 4: Commit**

```bash
git add src/zedsheet/responsive.rs src/zedsheet/zedsheet.rs
git commit -m "feat(mobile): tap-to-reveal popover for view-only mode

In view-only mode, tapping a cell shows a small overlay with the
cell's display value (or the underlying formula expression for
formula cells). Hides on the next tap or after 5 s. Replaces
the formula bar, which is hidden under view-only."
```

---

### Task 8: Manual browser verification + update BACKLOG

**Files:**
- Modify: `BACKLOG.md` (add a §6 row for mobile view-only with ✅)
- Modify: `CLAUDE.md` (optional — note that mobile support is gated on `Mode::ViewOnly`)

**Interfaces:**
- Consumes: nothing
- Produces: BACKLOG reflects the shipped feature; manual QA confirms the spec works in real browsers.

- [ ] **Step 1: Manual verification on desktop + mobile-emulated browsers**

Steps for the verifier (any engineer, no setup beyond the existing `trunk serve`):
1. `cargo build --target wasm32-unknown-unknown` — verify it compiles.
2. `trunk serve` (or `cargo build --target wasm32-unknown-unknown` + open `dist/index.html` via any static server).
3. **Desktop** (≥ 1024 px): confirm toolbar is unchanged; verify the three new test methods still work.
4. **Tablet** (768 px): use browser devtools mobile emulator (iPad). Verify toolbar drops labels, formula bar still shown.
5. **Phone-large** (375 px, iPhone): verify toolbar scrolls horizontally, formula bar hidden, tap targets ≥ 44 px, row/col headers shrink.
6. **Phone** (320 px): verify the further collapse.
7. **Long-press** (Chrome mobile emulator, touchscreen): tap and hold the canvas → context menu appears within ~ 500 ms. Drag away → no context menu.
8. **Pinch-zoom**: in mobile emulator, pinch the canvas → grid zooms, toolbar stays fixed.
9. **View-only**: call `mount(selector, { mode: "viewOnly" }, data)` (or extend the JS API to accept `mode: "viewOnly"`). Verify every editing affordance is hidden, tap-to-reveal popover works.

If any step fails, file a fix-task (don't patch in this task).

- [ ] **Step 2: Update BACKLOG.md**

Add to the §6 area (or a new "Mobile / multi-platform" section if you prefer — coordinate with the maintainer). Suggested entry:

```markdown
| | ~~**P3**~~ ✅ | Mobile view-only (read-only on phones + tablets) | Fixed 2026-07-20 (Phase 7): `Options.mode = Mode::ViewOnly` sets every sheet to read-only + `[data-readonly="true"]` on the root container. CSS @media rules at 1024 / 768 / 480 collapse the toolbar, hide the formula bar, shrink row/col headers, and pad every button to a 44 px tap target. Long-press synthesises `contextmenu` after 500 ms; pinch-zoom + Ctrl-wheel route to `renderer.set_zoom`. Tap-to-reveal popover replaces the formula bar in view-only mode. 4 new host tests (breakpoint_class, should_show_formula_bar, toolbar_button_subset, view_only_blocks). Editing on mobile is a separate BACKLOG entry. |
```

- [ ] **Step 3: Commit + push**

```bash
git add BACKLOG.md
git commit -m "docs(backlog): check off the mobile view-only feature

Cross-references the spec at docs/superpowers/specs/2026-07-20-
mobile-view-only-design.md and the 7 implementation commits.
Editing on mobile is the natural follow-up spec."
git push origin main
```

---

## Self-Review

Run after writing the plan. The skill lists 3 checks:

1. **Spec coverage** — Run through each spec section and point to a task that implements it:

| Spec section            | Task                                |
| ---------------------- | ----------------------------------- |
| §1 Architecture        | Tasks 1, 6                          |
| §2 Breakpoints + CSS   | Tasks 2 (decision fns), 3 (CSS)      |
| §3a Long-press         | Task 4                              |
| §3b Pinch-zoom         | Task 5                              |
| §3c Two-finger pan     | Implicit in Task 3 CSS               |
| §4 View-only           | Tasks 1, 6, 7                        |
| §5 Host tests          | Task 2                              |
| §5 Out-of-scope        | Explicit in Task 8                   |

2. **Placeholder scan** — No "TBD", "TODO" (in the plan body), "implement later", "fill in details" left. The word "TODO" only appears in the spec file at docs/superpowers/specs/, not in this plan.

3. **Type consistency** — Functions and types referenced match across tasks:
   - `Breakpoint` enum — defined in Task 2, used in Task 3 CSS only via the `toolbar_button_subset` return.
   - `view_only_blocks` — defined in Task 2, used in Tasks 6, 4 (via `wire_long_press` synthesises contextmenu; the existing context-menu.rs short-circuit at Task 6 step 2 uses it).
   - `set_read_only` — already exists on `DataProxy`; used in Task 1 + Task 6.
   - `set_zoom` — already exists on `TableRenderer`; used in Task 5.
   - `cell_display_value` — already exists on `DataProxy`; used in Task 7.
   - `SharedRenderer` — type alias for `Rc<RefCell<TableRenderer>>`; used in Tasks 5, 7.

4. **Spec coverage gaps** — None. Every section of the spec maps to at least one task.

---

**Plan written to `docs/superpowers/plans/2026-07-20-mobile-view-only.md`.**
