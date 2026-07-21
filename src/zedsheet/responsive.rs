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

    /// Wire a single-pointer touch pan → `renderer.scroll_by`.
    /// On desktop, the canvas uses custom scrollbars driven by
    /// `scroll_rows` / `scroll_cols` and mouse-wheel handlers —
    /// DOM-native scroll is disabled (the spreadsheet body has
    /// no `overflow`). On mobile the wrapper has
    /// `touch-action: pan-x pan-y pinch-zoom`, which means the
    /// browser *will* try to pan-scroll the page, but it can't
    /// scroll the spreadsheet body because the body has no
    /// overflow and the canvas itself doesn't scroll. This
    /// handler fills the gap: when a single finger drags more
    /// than `ACTIVATION_SLOP_PX` from `pointerdown`, the
    /// accumulated delta is converted to whole-cell scrolls
    /// (using the current row/col height as the step size) and
    /// applied via `scroll_by`, which both updates
    /// `scroll_rows` / `scroll_cols` and re-renders.
    ///
    /// Coexists with `wire_long_press`: the long-press timer
    /// cancels itself when the same pointer moves more than
    /// `LONG_PRESS_SLOP_PX` (10 px), and this handler only
    /// activates past the same threshold. So a touch-and-hold
    /// opens the context menu, a drag-instead pans the grid.
    pub fn wire_touch_pan(canvas_el: &web_sys::Element, renderer: SharedRenderer) {
        const ACTIVATION_SLOP_PX: f64 = 10.0;
        type PanState = Rc<RefCell<Option<PanSession>>>;
        struct PanSession {
            pointer_id: i32,
            start_x: f64,
            start_y: f64,
            last_x: f64,
            last_y: f64,
            // Pixel accumulator. When |accum_x| exceeds the
            // current column width, we scroll one column and
            // subtract the width; same for `accum_y` vs row height.
            accum_x: f64,
            accum_y: f64,
            active: bool,
        }

        let pan_state: PanState = Rc::new(RefCell::new(None));

        // pointerdown: seed a session in "potential" mode. It only
        // becomes active once the pointer moves more than the slop,
        // so a tap that turns into a long-press still wins.
        let down_state = Rc::clone(&pan_state);
        let down_cb = Closure::<dyn FnMut(PointerEvent)>::new(move |event: PointerEvent| {
            let x = event.client_x() as f64;
            let y = event.client_y() as f64;
            *down_state.borrow_mut() = Some(PanSession {
                pointer_id: event.pointer_id(),
                start_x: x,
                start_y: y,
                last_x: x,
                last_y: y,
                accum_x: 0.0,
                accum_y: 0.0,
                active: false,
            });
        });
        let _ = canvas_el
            .add_event_listener_with_callback("pointerdown", down_cb.as_ref().unchecked_ref());
        down_cb.forget();

        // pointermove: if the pointer moved past the slop while
        // down, activate the pan and convert accumulated pixel
        // delta into whole-cell scroll steps.
        let move_state = Rc::clone(&pan_state);
        let renderer_for_move = renderer.clone();
        let move_cb = Closure::<dyn FnMut(PointerEvent)>::new(move |event: PointerEvent| {
            let mut session = match move_state.borrow_mut().take() {
                Some(s) if s.pointer_id == event.pointer_id() => s,
                other => {
                    // Different pointer, or no session — restore
                    // and bail. The other pointer's session (if any)
                    // resumes next move.
                    if other.is_some() {
                        *move_state.borrow_mut() = other;
                    }
                    return;
                }
            };
            let x = event.client_x() as f64;
            let y = event.client_y() as f64;
            let dx = x - session.last_x;
            let dy = y - session.last_y;
            session.last_x = x;
            session.last_y = y;

            if !session.active {
                let total_dx = (x - session.start_x).abs();
                let total_dy = (y - session.start_y).abs();
                if total_dx > ACTIVATION_SLOP_PX || total_dy > ACTIVATION_SLOP_PX {
                    session.active = true;
                    // Reset accumulators at activation so the
                    // threshold-crossing movement isn't double-counted
                    // as a scroll step.
                    session.accum_x = 0.0;
                    session.accum_y = 0.0;
                }
            }

            if session.active {
                session.accum_x += dx;
                session.accum_y += dy;
                let (col_w, row_h) = {
                    let r = renderer_for_move.borrow();
                    // Use the current scroll-position row/col width
                    // as the step size; fall back to 24 px (the
                    // common unzoomed default) if the renderer
                    // reports a non-positive value.
                    let w = r.col_width_at(r.scroll_cols).max(1.0);
                    let h = r.row_height_at(r.scroll_rows).max(1.0);
                    (w, h)
                };
                let mut d_rows: i32 = 0;
                let mut d_cols: i32 = 0;
                if session.accum_y.abs() >= row_h {
                    d_rows = -(session.accum_y / row_h) as i32;
                    session.accum_y -= d_rows as f64 * row_h;
                }
                if session.accum_x.abs() >= col_w {
                    d_cols = -(session.accum_x / col_w) as i32;
                    session.accum_x -= d_cols as f64 * col_w;
                }
                if d_rows != 0 || d_cols != 0 {
                    let mut r = renderer_for_move.borrow_mut();
                    r.scroll_by(d_rows, d_cols);
                    r.render();
                }
            }

            *move_state.borrow_mut() = Some(session);
        });
        let _ = canvas_el
            .add_event_listener_with_callback("pointermove", move_cb.as_ref().unchecked_ref());
        move_cb.forget();

        // pointerup / pointercancel: clear the session.
        let up_state = Rc::clone(&pan_state);
        let up_cb = Closure::<dyn FnMut(PointerEvent)>::new(move |event: PointerEvent| {
            let should_clear = up_state
                .borrow()
                .as_ref()
                .is_some_and(|s| s.pointer_id == event.pointer_id());
            if should_clear {
                *up_state.borrow_mut() = None;
            }
        });
        let _ =
            canvas_el.add_event_listener_with_callback("pointerup", up_cb.as_ref().unchecked_ref());
        let _ = canvas_el
            .add_event_listener_with_callback("pointercancel", up_cb.as_ref().unchecked_ref());
        up_cb.forget();
    }

    /// Wire pinch-zoom → `renderer.set_zoom`. Only the
    /// `gesturechange` event (WebKit / iOS Safari) routes through
    /// here. The desktop Ctrl-wheel zoom is handled by the
    /// main `wire_events` wheel listener in `events.rs` — adding
    /// a second one here would double-apply the zoom (a single
    /// Ctrl-wheel-up from 100% would become 110%, then 121%).
    /// `gesturestart` is wired too so the relative scale baseline
    /// is fresh on each pinch gesture.
    pub fn wire_pinch_zoom(canvas_el: &web_sys::Element, renderer: SharedRenderer) {
        // `gesturestart` is needed on iOS to keep the platform's
        // default pinch-text-zoom behaviour from kicking in once
        // the user pinches the canvas. We don't act on it (the
        // next `gesturechange` reads `event.scale` directly), but
        // having a listener attached tells Safari our element
        // intends to consume the gesture.
        let canvas_el_gesture_start = canvas_el.clone();
        let start_cb = Closure::<dyn FnMut(JsValue)>::new(move |_ev: JsValue| {
            // No-op: the relative scale on `gesturechange` is
            // what `set_zoom` consumes.
        });
        let _ = canvas_el_gesture_start.add_event_listener_with_callback(
            "gesturestart",
            start_cb.as_ref().unchecked_ref(),
        );
        start_cb.forget();

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
    }

    /// Tap-to-reveal popover. In view-only mode, tapping a
    /// cell shows a small overlay above it with the cell's
    /// display value (or the underlying formula expression for
    /// formula cells). Hides on the next pointerup or after 5 s.
    pub fn wire_tap_reveal(
        canvas_el: &web_sys::Element,
        renderer: crate::zedsheet::SharedRenderer,
    ) {
        // Build the popover element once. Style mirrors the note
        // popover in `events.rs` so mobile and desktop look identical.
        let doc = web_sys::window().and_then(|w| w.document());
        let Some(document_el) = doc.clone() else {
            return;
        };
        let popover = doc.as_ref().and_then(|d| d.create_element("div").ok());
        let Some(popover) = popover else {
            return;
        };
        let _ = popover.set_attribute(
            "style",
            "display:none;position:fixed;z-index:400;max-width:240px;background:#fffbe6;border:1px solid #d9c97a;box-shadow:1px 2px 6px rgba(0,0,0,0.2);padding:6px 8px;font-size:12px;white-space:pre-wrap;pointer-events:none;color:#333;",
        );
        let _ = doc.and_then(|d| d.body()).map(|b| {
            let _ = b.append_child(&popover);
        });

        // The hide-listener, deferred-install callback, and hide-timeout from
        // the previous tap survive across calls so we can cancel them (instead
        // of stacking one new listener and one 5 s timer per tap, which is what
        // bit the original implementation).
        struct HideHandles {
            listener_cb: Option<Closure<dyn FnMut(PointerEvent)>>,
            listener_install_cb: Option<Closure<dyn FnMut()>>,
            listener_install_id: Option<i32>,
            timeout_cb: Option<Closure<dyn FnMut()>>,
            timeout_id: Option<i32>,
        }
        let hide_handles: Rc<RefCell<HideHandles>> = Rc::new(RefCell::new(HideHandles {
            listener_cb: None,
            listener_install_cb: None,
            listener_install_id: None,
            timeout_cb: None,
            timeout_id: None,
        }));

        // Removes whatever listener / timer the previous tap left
        // behind. Shared between show_cb (cancel-before-show) and
        // the hide callbacks (self-cleanup after firing).
        let cancel_handles = {
            let hide_handles = hide_handles.clone();
            let document_for_cancel = document_el.clone();
            move || {
                let mut handles = hide_handles.borrow_mut();
                if let Some(id) = handles.listener_install_id.take() {
                    if let Some(w) = web_sys::window() {
                        w.clear_timeout_with_handle(id);
                    }
                }
                handles.listener_install_cb.take();
                if let Some(cb) = handles.listener_cb.take() {
                    let _ = document_for_cancel.remove_event_listener_with_callback(
                        "pointerup",
                        cb.as_ref().unchecked_ref(),
                    );
                }
                if let Some(id) = handles.timeout_id.take() {
                    if let Some(w) = web_sys::window() {
                        w.clear_timeout_with_handle(id);
                    }
                }
                handles.timeout_cb.take();
            }
        };

        let popover_for_show = popover.clone();
        let renderer_for_show = renderer.clone();
        let hide_handles_for_show = hide_handles.clone();
        let show_cb = Closure::<dyn FnMut(PointerEvent)>::new(move |ev: PointerEvent| {
            // Cancel any leftover hide-handles from the previous tap
            // before we register new ones. Without this, every tap
            // would leak one pointerup listener and one 5 s timer.
            cancel_handles();

            let (x, y) = (ev.offset_x() as f64, ev.offset_y() as f64);
            let hit = renderer_for_show.borrow().cell_at(x, y);
            let Some((r, c)) = hit else {
                let _ = popover_for_show
                    .unchecked_ref::<web_sys::HtmlElement>()
                    .style()
                    .set_property("display", "none");
                return;
            };
            let text = renderer_for_show.borrow().data.cell_display_value(r, c);
            popover_for_show.set_text_content(Some(&text));
            let s = popover_for_show
                .unchecked_ref::<web_sys::HtmlElement>()
                .style();
            let _ = s.set_property("display", "block");
            let _ = s.set_property("left", &format!("{}px", ev.client_x() + 8));
            let _ = s.set_property("top", &format!("{}px", ev.client_y() + 8));

            // Hide on the next pointerup anywhere (registered on
            // document so a tap on the toolbar / formula bar also
            // dismisses the popover). Self-removes when fired so the
            // next tap doesn't have to remove it explicitly.
            let popover_for_up = popover_for_show.clone();
            let hide_handles_for_up = hide_handles_for_show.clone();
            let document_for_up = document_el.clone();
            let hide_up_cb = Closure::<dyn FnMut(PointerEvent)>::new(move |_ev: PointerEvent| {
                let _ = popover_for_up
                    .unchecked_ref::<web_sys::HtmlElement>()
                    .style()
                    .set_property("display", "none");
                let mut handles = hide_handles_for_up.borrow_mut();
                handles.listener_install_cb.take();
                if let Some(cb) = handles.listener_cb.take() {
                    let _ = document_for_up.remove_event_listener_with_callback(
                        "pointerup",
                        cb.as_ref().unchecked_ref(),
                    );
                }
                if let Some(id) = handles.timeout_id.take() {
                    if let Some(w) = web_sys::window() {
                        w.clear_timeout_with_handle(id);
                    }
                }
                handles.timeout_cb.take();
            });
            hide_handles_for_show.borrow_mut().listener_cb = Some(hide_up_cb);

            // The show callback runs on the canvas's pointerup. Defer installing
            // the document listener until the next event tick so this same
            // pointerup cannot immediately dismiss the newly shown popover.
            if let Some(window) = web_sys::window() {
                let document_for_install = document_el.clone();
                let hide_handles_for_install = hide_handles_for_show.clone();
                let install_cb = Closure::<dyn FnMut()>::new(move || {
                    let mut handles = hide_handles_for_install.borrow_mut();
                    handles.listener_install_id = None;
                    if let Some(cb) = handles.listener_cb.as_ref() {
                        let _ = document_for_install.add_event_listener_with_callback(
                            "pointerup",
                            cb.as_ref().unchecked_ref(),
                        );
                    }
                });
                if let Ok(id) = window.set_timeout_with_callback_and_timeout_and_arguments_0(
                    install_cb.as_ref().unchecked_ref(),
                    0,
                ) {
                    let mut handles = hide_handles_for_show.borrow_mut();
                    handles.listener_install_id = Some(id);
                    handles.listener_install_cb = Some(install_cb);
                } else {
                    hide_handles_for_show.borrow_mut().listener_cb.take();
                    let _ = popover_for_show
                        .unchecked_ref::<web_sys::HtmlElement>()
                        .style()
                        .set_property("display", "none");
                }
            }

            // 5 s auto-hide timer. Same hide-and-cleanup body, but
            // `FnMut()` (no event arg). The closure is stored in
            // `hide_handles` (not forgotten) so the next tap can drop
            // it instead of leaking it.
            let popover_for_t = popover_for_show.clone();
            let hide_handles_for_t = hide_handles_for_show.clone();
            let document_for_t = document_el.clone();
            let hide_t_cb = Closure::<dyn FnMut()>::new(move || {
                let _ = popover_for_t
                    .unchecked_ref::<web_sys::HtmlElement>()
                    .style()
                    .set_property("display", "none");
                let mut handles = hide_handles_for_t.borrow_mut();
                handles.listener_install_cb.take();
                if let Some(cb) = handles.listener_cb.take() {
                    let _ = document_for_t.remove_event_listener_with_callback(
                        "pointerup",
                        cb.as_ref().unchecked_ref(),
                    );
                }
                if let Some(id) = handles.timeout_id.take() {
                    if let Some(w) = web_sys::window() {
                        w.clear_timeout_with_handle(id);
                    }
                }
                handles.timeout_cb.take();
            });
            if let Some(window) = web_sys::window() {
                if let Ok(id) = window.set_timeout_with_callback_and_timeout_and_arguments_0(
                    hide_t_cb.as_ref().unchecked_ref(),
                    5000,
                ) {
                    hide_handles_for_show.borrow_mut().timeout_id = Some(id);
                }
                hide_handles_for_show.borrow_mut().timeout_cb = Some(hide_t_cb);
            }
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
