use std::cell::RefCell;
use std::rc::Rc;
use gloo::utils::{document, window};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::{HtmlElement, HtmlTextAreaElement, KeyboardEvent, MouseEvent, WheelEvent};
use crate::component::element::Element;
use crate::renderer::table_renderer::DragKind;
#[allow(unused_imports)]
use super::*;

pub(crate) fn wire_events(
    canvas_el: &mut Element,
    renderer: &SharedRenderer,
    textarea: &HtmlTextAreaElement,
    editing: &EditingCell,
    editor_error_node: Option<HtmlElement>,
    list_popover_node: Option<web_sys::Element>,
    list_popover_visible: Rc<RefCell<bool>>,
    filter_menu_node: Option<web_sys::Element>,
    filter_menu_visible: Rc<RefCell<bool>>,
    sync: &SyncFn,
) {
    let dragging = Rc::new(RefCell::new(false));
    let drag: Rc<RefCell<Option<DragState>>> = Rc::new(RefCell::new(None));

    // A floating popup that shows a cell's note on hover.
    let note_popup: web_sys::Element = {
        let el = document().create_element("div").unwrap();
        let _ = el.set_attribute(
            "style",
            "display:none;position:fixed;z-index:400;max-width:240px;background:#fffbe6;border:1px solid #d9c97a;box-shadow:1px 2px 6px rgba(0,0,0,0.2);padding:6px 8px;font-size:12px;white-space:pre-wrap;pointer-events:none;color:#333;",
        );
        document().body().unwrap().append_child(&el).unwrap();
        el
    };

    // mousedown: start a header-resize / scrollbar drag, or select a cell.
    {
        let renderer = renderer.clone();
        let textarea = textarea.clone();
        let editing = editing.clone();
        let editor_error = editor_error_node.clone();
        let dragging = dragging.clone();
        let drag = drag.clone();
        let sync = sync.clone();
        let list_popover = list_popover_node.clone();
        let list_popover_visible = list_popover_visible.clone();
        let filter_menu = filter_menu_node.clone();
        let filter_menu_visible = filter_menu_visible.clone();
        canvas_el.add_event_listener("mousedown", move |event: web_sys::Event| {
            let me: MouseEvent = event.dyn_into().unwrap();
            let (x, y) = (me.offset_x() as f64, me.offset_y() as f64);

            // Every canvas interaction below can scroll the viewport, resize
            // geometry, or move the selection out from under the open editor
            // overlay (whose position is fixed at `start_edit`), so commit it
            // before processing. On validation failure the editor stays open
            // (issue #9) and the interaction is swallowed so nothing shifts
            // under the invalid editor.
            if !reconcile_editor(&renderer, &textarea, editor_error.as_ref(), &editing) {
                return;
            }

            // Header boundary → start a resize.
            let resize = renderer.borrow().resize_target(x, y);
            if let Some(kind) = resize {
                let start_size = match kind {
                    DragKind::ColResize(ci) => renderer.borrow().col_width_at(ci),
                    DragKind::RowResize(ri) => renderer.borrow().row_height_at(ri),
                    _ => 0f64,
                };
                *drag.borrow_mut() = Some(DragState { kind, start_x: x, start_y: y, start_size });
                return;
            }

            // Scrollbar track → start a scroll drag and jump immediately.
            let sb = renderer.borrow().scrollbar_target(x, y);
            if let Some(kind) = sb {
                *drag.borrow_mut() = Some(DragState { kind, start_x: x, start_y: y, start_size: 0f64 });
                apply_scroll_drag(&renderer, kind, x, y);
                return;
            }

            // Fill handle → start a fill drag from the current selection.
            if renderer.borrow().is_on_fill_handle(x, y) {
                renderer.borrow_mut().start_fill();
                *drag.borrow_mut() =
                    Some(DragState { kind: DragKind::Fill, start_x: x, start_y: y, start_size: 0f64 });
                return;
            }

            // AutoFilter header glyph (issue #10): clicking the ▼ on a header
            // cell of the active filter range opens the filter menu. Deferred
            // a tick (like the list popover) so the global outside-click
            // closer, which sees this same mousedown, doesn't immediately
            // close the menu we just opened.
            let filter_hit = renderer.borrow().filter_glyph_hit(x, y);
            if let Some(f_ci) = filter_hit {
                let menu_for_open = filter_menu.clone();
                let renderer_for_open = renderer.clone();
                let visible_for_open = filter_menu_visible.clone();
                let cb = Closure::<dyn FnMut()>::new(move || {
                    show_filter_menu(
                        menu_for_open.as_ref(),
                        &renderer_for_open,
                        f_ci, x, y,
                        &visible_for_open,
                    );
                });
                if let Some(w) = web_sys::window() {
                    let _ = w.set_timeout_with_callback_and_timeout_and_arguments_0(
                        cb.as_ref().unchecked_ref(),
                        0,
                    );
                }
                cb.forget();
                return;
            }

            // List-validity glyph hit-test (issue #9): clicking the ▼ on a
            // list-valid cell opens the popover instead of starting a
            // selection. The glyph sits in the rightmost ~14px of the cell.
            // Compute hit-test details in a single borrow scope to avoid
            // overlapping immutable borrows on the renderer's RefCell.
            // NOTE: this is a `let = { ... }` block expression, so a bare
            // `return` here would exit the whole mousedown closure (and skip
            // cell selection below). Every non-glyph path must yield `None`.
            let glyph_hit: Option<(usize, usize, f64, f64)> = {
                let r = renderer.borrow();
                match r.cell_at(x, y) {
                    Some((ri, ci)) => {
                        let (origin_ri, origin_ci) = r.merge_origin(ri, ci);
                        if r.cell_has_list_validator(origin_ri, origin_ci) {
                            let rect = r.cell_screen_rect(origin_ri, origin_ci);
                            let in_glyph = x >= rect.x + rect.width - 17.0
                                && x <= rect.x + rect.width
                                && y >= rect.y
                                && y <= rect.y + rect.height;
                            if in_glyph {
                                Some((origin_ri, origin_ci, rect.x, rect.y))
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    }
                    None => None,
                }
            };
            if let Some((origin_ri, origin_ci, _rx, _ry)) = glyph_hit {
                // Select the cell and open the popover. We defer the
                // `visible=true` write to after this event loop tick so the
                // global "outside click" mousedown listener (which sees the
                // same event we just handled) bails on `visible == false`
                // rather than closing the popover we just opened.
                {
                    let mut r = renderer.borrow_mut();
                    r.select_cell(origin_ri, origin_ci);
                    r.render();
                }
                let popover_for_open = list_popover.clone();
                let renderer_for_open = renderer.clone();
                let visible_for_open = list_popover_visible.clone();
                let cb = Closure::<dyn FnMut()>::new(move || {
                    show_list_popover(
                        popover_for_open.as_ref(),
                        &renderer_for_open,
                        origin_ri, origin_ci, x, y,
                        &visible_for_open,
                    );
                });
                if let Some(w) = web_sys::window() {
                    let _ = w.set_timeout_with_callback_and_timeout_and_arguments_0(
                        cb.as_ref().unchecked_ref(),
                        0,
                    );
                }
                cb.forget();
                return;
            }

            // (Click-outside committed the open editor at the top of this
            // handler, so the click below always lands on a settled grid.)
            let hit = renderer.borrow().cell_at(x, y);
            if let Some((ri, ci)) = hit {
                // Ctrl/Cmd-click on a hyperlink cell follows the link.
                if me.ctrl_key() || me.meta_key() {
                    if let Some(url) = renderer.borrow().link_at(ri, ci) {
                        let _ = window().open_with_url_and_target(&url, "_blank");
                        return;
                    }
                }
                let mut r = renderer.borrow_mut();
                if me.ctrl_key() || me.meta_key() {
                    // Issue #19: Ctrl/Cmd-click adds a disjoint range. If the
                    // click landed inside an existing range, do nothing
                    // (matches Excel — toggling disjoint selection).
                    if !r.contains_selected(ri, ci) {
                        // First Ctrl+click: promote the current single-rect
                        // selection to a multi-range entry so the user's
                        // first picked cell stays selected.
                        if !r.multi_range_is_active() {
                            r.promote_selector_to_range();
                        }
                        let (sr, sc) = r.merge_origin(ri, ci);
                        r.add_range(sr, sc, sr, sc);
                    }
                } else {
                    // Plain click clears any Ctrl/Cmd-added ranges and
                    // starts a new single-rect selection.
                    r.clear_multi_range();
                    r.select_cell(ri, ci);
                }
                r.render();
                *dragging.borrow_mut() = true;
                drop(r);
                sync();
            }
        });
    }

    // mousemove: apply an active drag, extend a selection, or update the cursor.
    {
        let renderer = renderer.clone();
        let dragging = dragging.clone();
        let drag = drag.clone();
        let note_popup = note_popup.clone();
        canvas_el.add_event_listener("mousemove", move |event: web_sys::Event| {
            let me: MouseEvent = event.dyn_into().unwrap();
            let (x, y) = (me.offset_x() as f64, me.offset_y() as f64);

            // Active header-resize / scrollbar drag.
            if let Some(ds) = *drag.borrow() {
                hide_tooltip(&note_popup);
                let mut r = renderer.borrow_mut();
                match ds.kind {
                    DragKind::ColResize(ci) => {
                        r.set_col_width_clamped(ci, ds.start_size + (x - ds.start_x));
                        r.render();
                    }
                    DragKind::RowResize(ri) => {
                        r.set_row_height_clamped(ri, ds.start_size + (y - ds.start_y));
                        r.render();
                    }
                    DragKind::VScroll | DragKind::HScroll => {
                        drop(r);
                        apply_scroll_drag(&renderer, ds.kind, x, y);
                    }
                    DragKind::Fill => {
                        // Extend the selection toward the cursor as a fill preview.
                        if let Some((ri, ci)) = r.cell_at(x, y) {
                            r.select_to(ri, ci);
                            r.render();
                        }
                    }
                }
                return;
            }

            // Drag-select.
            if *dragging.borrow() {
                hide_tooltip(&note_popup);
                let hit = renderer.borrow().cell_at(x, y);
                if let Some((ri, ci)) = hit {
                    let mut r = renderer.borrow_mut();
                    if me.ctrl_key() || me.meta_key() {
                        // Issue #19: extend only the most-recently-added range
                        // when Ctrl/Cmd is held during drag.
                        r.select_to_last(ri, ci);
                    } else {
                        r.select_to(ri, ci);
                    }
                    r.render();
                }
                return;
            }

            // Hover feedback: resize cursor near header boundaries.
            {
                let r = renderer.borrow();
                match r.resize_target(x, y) {
                    Some(DragKind::ColResize(_)) => r.set_cursor("col-resize"),
                    Some(DragKind::RowResize(_)) => r.set_cursor("row-resize"),
                    _ => r.set_cursor("default"),
                }
            }

            // Note popup: show the hovered cell's note (if any).
            let note = renderer
                .borrow()
                .cell_at(x, y)
                .and_then(|(ri, ci)| renderer.borrow().note_at(ri, ci));
            match note {
                Some(text) => {
                    note_popup.set_text_content(Some(&text));
                    let style = note_popup.unchecked_ref::<web_sys::HtmlElement>().style();
                    let _ = style.set_property("left", &format!("{}px", me.client_x() + 12));
                    let _ = style.set_property("top", &format!("{}px", me.client_y() + 12));
                    let _ = style.set_property("display", "block");
                }
                None => hide_tooltip(&note_popup),
            }
        });
    }

    // dblclick: edit the clicked cell.
    {
        let renderer = renderer.clone();
        let textarea = textarea.clone();
        let editing = editing.clone();
        let editor_error = editor_error_node.clone();
        canvas_el.add_event_listener("dblclick", move |event: web_sys::Event| {
            let me: MouseEvent = event.dyn_into().unwrap();
            let (x, y) = (me.offset_x() as f64, me.offset_y() as f64);
            let hit = renderer.borrow().cell_at(x, y);
            if let Some((ri, ci)) = hit {
                // Edit the merge origin when the cell is part of a merge.
                let (ri, ci) = renderer.borrow().merge_origin(ri, ci);
                start_edit(&renderer, &textarea, editor_error.as_ref(), &editing, ri, ci);
            }
        });
    }

    // wheel: scroll the body by whole cells.
    {
        let renderer = renderer.clone();
        let textarea = textarea.clone();
        let editing = editing.clone();
        let editor_error = editor_error_node.clone();
        canvas_el.add_event_listener("wheel", move |event: web_sys::Event| {
            let we: WheelEvent = event.clone().dyn_into().unwrap();
            we.prevent_default();
            // Scrolling re-renders the grid (and its canvas-drawn selection)
            // at new screen positions while the absolutely-positioned editor
            // would stay at stale pixels — commit it first. An invalid value
            // keeps the editor open and swallows the scroll (issue #9).
            if !reconcile_editor(&renderer, &textarea, editor_error.as_ref(), &editing) {
                return;
            }
            let dy = we.delta_y();
            let dx = we.delta_x();
            let d_rows = if dy > 0.0 { 1 } else if dy < 0.0 { -1 } else { 0 };
            let d_cols = if dx > 0.0 { 1 } else if dx < 0.0 { -1 } else { 0 };
            if d_rows != 0 || d_cols != 0 {
                let mut r = renderer.borrow_mut();
                r.scroll_by(d_rows, d_cols);
                r.render();
            }
        });
    }

    // window keydown: arrow navigation + Enter-to-edit when not editing.
    {
        let renderer = renderer.clone();
        let textarea = textarea.clone();
        let editing = editing.clone();
        let editor_error = editor_error_node.clone();
        let sync = sync.clone();
        let cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
            if editing.borrow().is_some() {
                return; // cell editor handles its own keys while editing
            }
            // Ignore grid keys while a formula-bar input (name box / formula
            // input) is focused — those inputs handle their own keystrokes.
            if let Some(active) = document().active_element() {
                if active.tag_name().eq_ignore_ascii_case("input") {
                    return;
                }
            }
            let ke: KeyboardEvent = event.dyn_into().unwrap();
            let key = ke.key();

            // Ctrl/Cmd shortcuts: style toggles + undo/redo. Copy/cut/paste are
            // handled by the native clipboard events in `system_clipboard` so
            // they also reach Excel and other apps — they are intentionally not
            // intercepted here (no `prevent_default`, so the browser dispatches
            // the `copy`/`cut`/`paste` events we listen for).
            if ke.ctrl_key() || ke.meta_key() {
                let mut handled = true;
                {
                    let mut r = renderer.borrow_mut();
                    match key.to_lowercase().as_str() {
                        "b" => r.toggle_bold(),
                        "i" => r.toggle_italic(),
                        "u" => r.toggle_underline(),
                        // Ctrl/Cmd+Z undo; Ctrl/Cmd+Y or Ctrl/Cmd+Shift+Z redo.
                        "z" if ke.shift_key() => r.redo(),
                        "z" => r.undo(),
                        "y" => r.redo(),
                        _ => handled = false,
                    }
                    if handled {
                        r.render();
                    }
                }
                if handled {
                    ke.prevent_default();
                    sync();
                }
                return;
            }

            // Delete/Backspace clears the selected cells.
            if key == "Delete" || key == "Backspace" {
                {
                    let mut r = renderer.borrow_mut();
                    r.clear_selection_content();
                    r.render();
                }
                ke.prevent_default();
                sync();
                return;
            }

            let (mut dr, mut dc) = (0i32, 0i32);
            match key.as_str() {
                "ArrowUp" => dr = -1,
                "ArrowDown" => dr = 1,
                "ArrowLeft" => dc = -1,
                "ArrowRight" => dc = 1,
                "Enter" | "F2" => {
                    let (ri, ci) = {
                        let r = renderer.borrow();
                        let s = r.get_selector();
                        (s.ri, s.ci)
                    };
                    start_edit(&renderer, &textarea, editor_error.as_ref(), &editing, ri, ci);
                    ke.prevent_default();
                    return;
                }
                _ => return,
            }
            ke.prevent_default();
            {
                let mut r = renderer.borrow_mut();
                r.move_selection(dr, dc);
                r.render();
            }
            sync();
        });
        window()
            .add_event_listener_with_callback("keydown", cb.as_ref().unchecked_ref())
            .unwrap();
        cb.forget();
    }

    // textarea keydown: commit/cancel while editing.
    {
        let renderer = renderer.clone();
        let textarea_inner = textarea.clone();
        let editing = editing.clone();
        let editor_error = editor_error_node.clone();
        let sync = sync.clone();
        let cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
            let ke: KeyboardEvent = event.dyn_into().unwrap();
            match ke.key().as_str() {
                "Enter" => {
                    ke.prevent_default();
                    ke.stop_propagation();
                    // Issue #9: on validation failure, keep the editor open
                    // and skip the selection move so the user can fix the
                    // value in place.
                    if commit_edit(&renderer, &textarea_inner, editor_error.as_ref(), &editing).is_ok() {
                        let mut r = renderer.borrow_mut();
                        r.move_selection(1, 0);
                        r.render();
                    }
                    sync();
                }
                "Tab" => {
                    ke.prevent_default();
                    ke.stop_propagation();
                    if commit_edit(&renderer, &textarea_inner, editor_error.as_ref(), &editing).is_ok() {
                        let mut r = renderer.borrow_mut();
                        r.move_selection(0, 1);
                        r.render();
                    }
                    sync();
                }
                "Escape" => {
                    ke.prevent_default();
                    ke.stop_propagation();
                    cancel_edit(&textarea_inner, editor_error.as_ref(), &editing);
                }
                _ => {
                    ke.stop_propagation();
                }
            }
        });
        textarea
            .add_event_listener_with_callback("keydown", cb.as_ref().unchecked_ref())
            .unwrap();
        cb.forget();
    }

    // window mouseup: end drag-select and any header/scrollbar/fill drag.
    {
        let dragging = dragging.clone();
        let drag = drag.clone();
        let renderer = renderer.clone();
        let sync = sync.clone();
        let cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |_event: web_sys::Event| {
            // A fill-handle drag applies the fill on release.
            let was_fill = matches!(*drag.borrow(), Some(ds) if ds.kind == DragKind::Fill);
            if was_fill {
                {
                    let mut r = renderer.borrow_mut();
                    r.apply_fill();
                    r.render();
                }
                sync();
            }
            *dragging.borrow_mut() = false;
            *drag.borrow_mut() = None;
        });
        window()
            .add_event_listener_with_callback("mouseup", cb.as_ref().unchecked_ref())
            .unwrap();
        cb.forget();
    }

    // System-clipboard glue: copy/cut/paste between the grid and other apps
    // (Excel, Google Sheets) plus lossless in-app round-trips.
    system_clipboard::install(canvas_el, renderer, editing, sync);
}

/// Map a scrollbar pointer position to a scroll fraction and apply it.
pub(crate) fn apply_scroll_drag(renderer: &SharedRenderer, kind: DragKind, x: f64, y: f64) {
    let mut r = renderer.borrow_mut();
    let (w, h, hw, ch) = {
        // width, height, row-header width, col-header height
        (r.width, r.height, r.row_header.width, r.col_header.height)
    };
    match kind {
        DragKind::VScroll => {
            let track = (h - ch).max(1f64);
            r.scroll_to_fraction_v((y - ch) / track);
        }
        DragKind::HScroll => {
            let track = (w - hw).max(1f64);
            r.scroll_to_fraction_h((x - hw) / track);
        }
        _ => {}
    }
    r.render();
}
