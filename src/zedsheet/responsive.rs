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

/// True when the formula bar should render. The bar is hidden
/// at < 768 px (saving vertical space on phones + tablets), and
/// always hidden in view-only mode (the cell editor is suppressed,
/// so the bar would be dead UI).
pub fn should_show_formula_bar(width: u32, view_only: bool) -> bool {
    !view_only && width >= 768
}

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
        "undo",
        "redo",
        "print",
        "font-bold",
        "font-italic",
        "underline",
        "strike",
        "color",
        "bgcolor",
        "merge",
        "borders",
        "halign",
        "valign",
        "textwrap",
        "freeze",
        "autofilter",
        "formula",
    ];
    const TABLET: &[&str] = &[
        "undo",
        "redo",
        "print",
        "font-bold",
        "font-italic",
        "underline",
        "freeze",
        "autofilter",
        "formula",
    ];
    const PHONE_LARGE: &[&str] = &["undo", "redo", "print", "freeze", "autofilter", "formula"];
    const PHONE: &[&str] = &["print", "autofilter", "formula"];
    match breakpoint_class(width) {
        Breakpoint::Desktop => DESKTOP,
        Breakpoint::Tablet => TABLET,
        Breakpoint::PhoneLarge => PHONE_LARGE,
        Breakpoint::Phone => PHONE,
    }
}

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

#[cfg(target_arch = "wasm32")]
pub mod wasm {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    use wasm_bindgen::closure::Closure;
    use wasm_bindgen::{JsCast, JsValue};
    use wasm_bindgen_futures::{spawn_local, JsFuture};
    use web_sys::PointerEvent;

    use crate::zedsheet::SharedRenderer;

    const LONG_PRESS_MS: i32 = 500;
    const LONG_PRESS_SLOP_PX: f64 = 10.0;
    type LongPressState = Rc<RefCell<Option<(f64, f64, i32, i32)>>>;

    fn timeout_promise(timeout_ms: i32) -> js_sys::Promise {
        js_sys::Promise::new(&mut |resolve, reject| {
            let resolve_on_timeout = resolve.clone();
            let callback = Closure::once_into_js(move || {
                let _ = resolve_on_timeout.call0(&JsValue::UNDEFINED);
            });
            let scheduled = web_sys::window().is_some_and(|window| {
                window
                    .set_timeout_with_callback_and_timeout_and_arguments_0(
                        callback.unchecked_ref(),
                        timeout_ms,
                    )
                    .is_ok()
            });
            if !scheduled {
                let reason = JsValue::from_str("failed to schedule long-press timeout");
                let _ = reject.call1(&JsValue::UNDEFINED, &reason);
            }
        })
    }

    fn dispatch_context_menu(canvas_el: &web_sys::Element, client_x: f64, client_y: f64) {
        // Browsers derive offsetX/offsetY from these viewport coordinates and
        // the dispatch target, which keeps the existing canvas hit-test working.
        let init = web_sys::MouseEventInit::new();
        init.set_bubbles(true);
        init.set_cancelable(true);
        init.set_client_x(client_x as i32);
        init.set_client_y(client_y as i32);
        init.set_button(2);
        if let Ok(event) = web_sys::MouseEvent::new_with_mouse_event_init_dict("contextmenu", &init)
        {
            let _ = canvas_el.dispatch_event(&event);
        }
    }

    /// Wire a long-press → contextmenu gesture on `canvas_el`.
    /// Touch has no right-click; we synthesise a `contextmenu`
    /// event after 500 ms of no-movement so the existing
    /// right-click handler runs unchanged.
    pub fn wire_long_press(canvas_el: &web_sys::Element) {
        let down_state: LongPressState = Rc::new(RefCell::new(None));
        let press_sequence = Rc::new(Cell::new(0_i32));

        let canvas_for_down = canvas_el.clone();
        let state_for_down = Rc::clone(&down_state);
        let sequence_for_down = Rc::clone(&press_sequence);
        let down_cb = Closure::<dyn FnMut(PointerEvent)>::new(move |event: PointerEvent| {
            let token = sequence_for_down.get().wrapping_add(1);
            sequence_for_down.set(token);
            let pending = (
                event.client_x() as f64,
                event.client_y() as f64,
                event.pointer_id(),
                token,
            );
            *state_for_down.borrow_mut() = Some(pending);

            let timer_state = Rc::clone(&state_for_down);
            let canvas_for_timer = canvas_for_down.clone();
            spawn_local(async move {
                if JsFuture::from(timeout_promise(LONG_PRESS_MS))
                    .await
                    .is_err()
                {
                    return;
                }
                let should_fire = {
                    let mut state = timer_state.borrow_mut();
                    if state.as_ref().is_some_and(|current| *current == pending) {
                        state.take();
                        true
                    } else {
                        false
                    }
                };
                if should_fire {
                    dispatch_context_menu(&canvas_for_timer, pending.0, pending.1);
                }
            });
        });
        let _ = canvas_el
            .add_event_listener_with_callback("pointerdown", down_cb.as_ref().unchecked_ref());
        down_cb.forget();

        // Moving more than 10 px means this is a drag, not a long-press.
        let move_state = Rc::clone(&down_state);
        let move_cb = Closure::<dyn FnMut(PointerEvent)>::new(move |event: PointerEvent| {
            let pending = *move_state.borrow();
            if let Some((x0, y0, pointer_id, _)) = pending {
                if pointer_id != event.pointer_id() {
                    return;
                }
                let dx = (event.client_x() as f64 - x0).abs();
                let dy = (event.client_y() as f64 - y0).abs();
                if dx > LONG_PRESS_SLOP_PX || dy > LONG_PRESS_SLOP_PX {
                    *move_state.borrow_mut() = None;
                }
            }
        });
        let _ = canvas_el
            .add_event_listener_with_callback("pointermove", move_cb.as_ref().unchecked_ref());
        move_cb.forget();

        // Releasing the initiating pointer cancels the pending long-press.
        let up_state = Rc::clone(&down_state);
        let up_cb = Closure::<dyn FnMut(PointerEvent)>::new(move |event: PointerEvent| {
            let is_active_pointer = up_state
                .borrow()
                .as_ref()
                .is_some_and(|pending| pending.2 == event.pointer_id());
            if is_active_pointer {
                *up_state.borrow_mut() = None;
            }
        });
        let _ =
            canvas_el.add_event_listener_with_callback("pointerup", up_cb.as_ref().unchecked_ref());
        let _ = canvas_el
            .add_event_listener_with_callback("pointercancel", up_cb.as_ref().unchecked_ref());
        up_cb.forget();
    }

    /// Wire pinch-zoom → `renderer.set_zoom`. Both the
    /// `gesturechange` event (WebKit / iOS Safari) and the
    /// `wheel + ctrlKey` event (desktop) route through the
    /// same zoom handler so the desktop Ctrl-wheel zoom and
    /// the mobile pinch zoom share a single scale model.
    pub fn wire_pinch_zoom(canvas_el: &web_sys::Element, renderer: SharedRenderer) {
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
                    r.render();
                }
            }
        });
        let _ = canvas_el_gesture
            .add_event_listener_with_callback("gesturechange", gesture_cb.as_ref().unchecked_ref());
        gesture_cb.forget();

        let canvas_el_wheel = canvas_el.clone();
        let renderer_wheel = renderer.clone();
        let wheel_cb =
            Closure::<dyn FnMut(web_sys::WheelEvent)>::new(move |ev: web_sys::WheelEvent| {
                if !ev.ctrl_key() {
                    return;
                }
                let current = renderer_wheel.borrow().zoom();
                let scale = if ev.delta_y() < 0.0 { 1.1 } else { 1.0 / 1.1 };
                let next = (current * scale).clamp(0.1, 4.0);
                let mut r = renderer_wheel.borrow_mut();
                r.set_zoom(next);
                r.render();
            });
        let _ = canvas_el_wheel
            .add_event_listener_with_callback("wheel", wheel_cb.as_ref().unchecked_ref());
        wheel_cb.forget();
    }

    /// Tap-to-reveal popover. In view-only mode, tapping a
    /// cell shows a small overlay above it with the cell's
    /// display value (or the underlying formula expression for
    /// formula cells). Hides on the next tap or after 5 s.
    pub fn wire_tap_reveal(
        canvas_el: &web_sys::Element,
        renderer: crate::zedsheet::SharedRenderer,
    ) {
        // Build the popover element once.
        let doc = web_sys::window().and_then(|w| w.document());
        let popover = doc.as_ref().and_then(|d| d.create_element("div").ok());
        let Some(popover) = popover else {
            return;
        };
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
            let Some((_r, _c)) = hit else {
                return;
            };
            let text = renderer_for_show.borrow().data.cell_display_value(_r, _c);
            popover_for_show.set_text_content(Some(&text));
            let s = popover_for_show
                .unchecked_ref::<web_sys::HtmlElement>()
                .style();
            let _ = s.set_property("display", "block");
            let _ = s.set_property("left", &format!("{}px", ev.client_x() + 8));
            let _ = s.set_property("top", &format!("{}px", ev.client_y() + 8));
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
            let _ = canvas_el_for_show
                .add_event_listener_with_callback("pointerup", next_up.as_ref().unchecked_ref());
            next_up.forget();
        });
        let _ = canvas_el
            .add_event_listener_with_callback("pointerup", show_cb.as_ref().unchecked_ref());
        show_cb.forget();
    }
}

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
        assert_eq!(breakpoint_class(768), Breakpoint::Tablet); // boundary
        assert_eq!(breakpoint_class(767), Breakpoint::PhoneLarge);
        assert_eq!(breakpoint_class(480), Breakpoint::PhoneLarge); // boundary
        assert_eq!(breakpoint_class(479), Breakpoint::Phone);
        assert_eq!(breakpoint_class(360), Breakpoint::Phone);
    }

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

    #[cfg(target_arch = "wasm32")]
    #[test]
    fn long_press_wiring_has_required_signature() {
        let _: fn(&web_sys::Element) = super::wasm::wire_long_press;
    }

    #[test]
    fn view_only_blocks_editing_actions_only() {
        // Blocked: every action that mutates data.
        for a in [
            "edit",
            "copy",
            "cut",
            "paste",
            "insert-row",
            "delete-col",
            "font-bold",
            "color",
            "merge",
            "condfmt",
            "chart",
            "image",
            "shape",
        ] {
            assert!(view_only_blocks(a), "{a} should be blocked in view-only");
        }
        // Not blocked: navigation + read-only actions.
        for a in ["print", "autofilter", "formula"] {
            assert!(
                !view_only_blocks(a),
                "{a} should remain enabled in view-only"
            );
        }
        // Unknown action: not blocked (default = allow).
        assert!(!view_only_blocks("does-not-exist"));
    }
}
