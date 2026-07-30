# Mobile editing (Phase 8)

> Follow-up #1 named in
> `docs/superpowers/specs/2026-07-20-mobile-view-only-design.md`:
> *"Casual editing on mobile — extends view-only with single-cell
> editing + virtual keyboard handling."*
>
> Phase 7 shipped the read-only foundation. This phase makes
> `Mode::Normal` actually usable on a touch device.

## Decisions

Settled before implementation:

| Question | Decision | Why |
|---|---|---|
| Edit entry gesture | **Double-tap the cell** | Mirrors the existing desktop `dblclick` path (`events.rs:387`) and matches Sheets / Numbers. Single tap keeps selecting. |
| Text input surface on phones | **Reveal the formula bar while editing** | The bar is a built 662-line component already wired to the editor. Revealing it only during an edit costs one CSS rule + one attribute, and gives full-width room for formulas. |
| Mode architecture | **Make `Mode::Normal` touch-capable** | No third variant. `ViewOnly` already covers read-only; a `CasualEdit` tier would duplicate `Normal`'s gating through `events.rs` and the toolbar. Touch behaviour keys off viewport width, not mode. |
| v1 scope beyond typing | **Virtual-keyboard viewport handling only** | Without it, editing any cell in the lower half of the screen is unusable. Touch range-selection handles, IME/composition handling, and a mobile undo affordance are explicitly deferred. |

## Non-goals (this phase)

- **Touch range selection / drag handles.** Multi-cell styling and the
  fill handle stay desktop-only. Separate BACKLOG entry.
- **IME / composition events.** CJK input may commit mid-composition.
  Separate BACKLOG entry — it needs its own `compositionstart` /
  `compositionend` handling in the commit path.
- **Mobile undo/redo affordance.** `Ctrl+Z` has no touch equivalent.
  Separate BACKLOG entry.
- **Accessibility**, **native wrapper**, **offline cache**, **collab** —
  already listed as separate entries by the Phase 7 spec.

## Design

### 1. Double-tap → synthetic `dblclick`

`events.rs:387` already binds `dblclick` on the canvas and opens the
editor (falling back to auto-fit on a header boundary). Rather than
duplicating that logic for touch, `wire_double_tap` synthesises a
`dblclick` `MouseEvent` at the tap coordinates — exactly the trick
`wire_long_press` uses for `contextmenu` (`responsive.rs:169`). The
entire existing edit path then runs unchanged.

Coexistence with the Phase 7 gestures:

- **Long-press (500 ms)** fires only if the pointer *doesn't* lift. A
  double-tap lifts twice well inside that window, so the long-press
  timer is cancelled by its own `pointerup` handler.
- **Touch pan** activates past 10 px of movement. Double-tap requires
  both taps within `DOUBLE_TAP_SLOP_PX` (24 px) of each other, and each
  individual tap is a no-move press, so a pan never satisfies it.

### 2. Formula bar reveal

`src/index.css:1068` hides `.zedsheet-formula-bar` inside
`@media (max-width: 767px)`. The mount root gains `data-editing="true"`
while an edit is in flight, and a companion rule inside the same media
block un-hides the bar. The attribute is mirrored from the three
existing editing transitions in `formula_bar.rs` — `start_edit` (579),
`commit_edit` (594 / 617), `cancel_edit` (656) — via a helper that
walks up from the textarea with `closest()`, so no new parameter has to
be threaded through every call site.

`should_show_formula_bar` gains an `editing` parameter to keep the pure
decision layer honest about the new behaviour.

### 3. Virtual keyboard viewport handling

`window.visualViewport` shrinks when the keyboard opens and emits
`resize`. On each resize while editing, the editor's rect is compared
against the still-visible band and the grid is scrolled by
`keyboard_scroll_delta`, applied through the existing
`renderer.scroll_by` (the same entry point Phase 7's touch pan uses).

The delta is a pure function so the geometry is host-testable:

- cell below the band → scroll down by the overflow, clamped so the
  cell's top never leaves the band
- cell above the band → scroll up by the shortfall
- cell taller than the band, or already visible → no scroll

## Work breakdown

| # | Commit | Scope |
|---|---|---|
| 1 | `feat(mobile): pure decision fns for touch editing` | `should_show_formula_bar` gains `editing`; new `is_double_tap` + `keyboard_scroll_delta`. Tests first. |
| 2 | `feat(mobile): double-tap → synthetic dblclick opens the editor` | `wire_double_tap` in `responsive::wasm` + wiring in `mod.rs`. |
| 3 | `feat(mobile): reveal the formula bar while editing on phones` | `data-editing` attribute mirrored from the three transitions + the CSS rule. |
| 4 | `feat(mobile): keep the edited cell above the virtual keyboard` | `visualViewport` listener + `keyboard_scroll_delta` application. |
| 5 | `docs(backlog): check off mobile editing` | BACKLOG §9 row + the deferred follow-ups. |

## Verification

- Host tests for all three pure functions (thresholds, clamping,
  view-only interaction).
- `cargo clippy --target wasm32-unknown-unknown --all-targets
  -- -D warnings` stays clean.
- `cargo build --target wasm32-unknown-unknown` ships.
- The DOM wiring can't run on the host target (same constraint Phase 7
  hit); signature-pinning tests guard the wiring entry points, and real
  verification is manual browser QA on a phone viewport.

## Risks

1. **Synthetic `dblclick` lands on a header boundary.** The existing
   handler auto-fits instead of editing when the tap is within 4 px of a
   boundary. On touch that tolerance is easy to hit by accident — worth
   watching in QA, but it mirrors desktop behaviour rather than
   inventing a new rule.
2. **`visualViewport` is absent in older browsers.** The listener is
   feature-detected; without it, editing still works, the cell just may
   sit under the keyboard.
3. **Double-tap vs. browser zoom.** Mobile browsers use double-tap to
   zoom. The existing `touch-action` on the wrapper already suppresses
   the default double-tap-zoom on the canvas; confirm in QA on iOS.
