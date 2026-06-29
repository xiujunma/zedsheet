#[allow(unused_imports)]
use super::*;
use crate::component::element::Element;
use crate::renderer::table_renderer::{DragKind, PasteMode};
use gloo::utils::{document, window};
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::{HtmlElement, HtmlTextAreaElement, KeyboardEvent, MouseEvent, WheelEvent};

pub(crate) fn wire_events(
    canvas_el: &mut Element,
    renderer: &SharedRenderer,
    sheets: &SheetsRegistry,
    active: &ActiveSheet,
    textarea: &HtmlTextAreaElement,
    editing: &EditingCell,
    editor_error_node: Option<HtmlElement>,
    list_popover_node: Option<web_sys::Element>,
    list_popover_visible: Rc<RefCell<bool>>,
    filter_menu_node: Option<web_sys::Element>,
    filter_menu_visible: Rc<RefCell<bool>>,
    sync: &SyncFn,
    delete_open: OpenHandle,
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

            // Outline gutter toggles / level buttons (issue #30). Checked
            // before resize: the widened header band overlaps the gutter, and
            // a toggle near a row boundary must win over a resize grab.
            let outline = renderer.borrow().outline_hit(x, y);
            if let Some(hit) = outline {
                {
                    let mut r = renderer.borrow_mut();
                    r.toggle_outline(hit);
                    r.render();
                }
                sync(); // hide flags are document state — persist + notify
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
                *drag.borrow_mut() = Some(DragState {
                    kind,
                    start_x: x,
                    start_y: y,
                    start_size,
                });
                return;
            }

            // Scrollbar track → start a scroll drag and jump immediately.
            let sb = renderer.borrow().scrollbar_target(x, y);
            if let Some(kind) = sb {
                *drag.borrow_mut() = Some(DragState {
                    kind,
                    start_x: x,
                    start_y: y,
                    start_size: 0f64,
                });
                apply_scroll_drag(&renderer, kind, x, y);
                return;
            }

            // Fill handle → start a fill drag from the current selection.
            if renderer.borrow().is_on_fill_handle(x, y) {
                renderer.borrow_mut().start_fill();
                *drag.borrow_mut() = Some(DragState {
                    kind: DragKind::Fill,
                    start_x: x,
                    start_y: y,
                    start_size: 0f64,
                });
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
                        f_ci,
                        x,
                        y,
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
                        origin_ri,
                        origin_ci,
                        x,
                        y,
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
                start_edit(
                    &renderer,
                    &textarea,
                    editor_error.as_ref(),
                    &editing,
                    ri,
                    ci,
                );
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
            // Ctrl/Cmd+wheel zooms in 10% steps (issue #32). prevent_default
            // (already called above) stops the browser's own page zoom.
            if we.ctrl_key() || we.meta_key() {
                let pct = {
                    let mut r = renderer.borrow_mut();
                    let step = if we.delta_y() < 0.0 { 0.1 } else { -0.1 };
                    let next = r.zoom() + step;
                    r.set_zoom(next);
                    r.render();
                    (r.zoom() * 100.0).round()
                };
                // Keep the toolbar's zoom dropdown title in sync.
                if let Some(t) = document().get_element_by_id("zs-dd-zoom") {
                    t.set_text_content(Some(&format!("{}%", pct)));
                }
                return;
            }
            let dy = we.delta_y();
            let dx = we.delta_x();
            let d_rows = if dy > 0.0 {
                1
            } else if dy < 0.0 {
                -1
            } else {
                0
            };
            let d_cols = if dx > 0.0 {
                1
            } else if dx < 0.0 {
                -1
            } else {
                0
            };
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
        let sheets = sheets.clone();
        let active = active.clone();
        let textarea = textarea.clone();
        let editing = editing.clone();
        let editor_error = editor_error_node.clone();
        let sync = sync.clone();
        let delete_open = delete_open.clone();
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
                let mut show_delete_dialog = false;
                {
                    let mut r = renderer.borrow_mut();
                    match key.to_lowercase().as_str() {
                        "b" => r.toggle_bold(),
                        "i" => r.toggle_italic(),
                        "u" => r.toggle_underline(),
                        // Ctrl/Cmd+Shift+V pastes values only (Google Sheets
                        // convention, issue #28). Plain Ctrl/Cmd+V is left to the
                        // native paste event in `system_clipboard`.
                        "v" if ke.shift_key() => r.paste_special(PasteMode::Values),
                        // Ctrl/Cmd+Z undo; Ctrl/Cmd+Y or Ctrl/Cmd+Shift+Z redo.
                        // Pivot operations (issue #35, issue #53) change the
                        // workbook's sheets registry, so the undo must also
                        // restore `sheets` + `active` from the saved
                        // `PivotUndo` record, not just `self.data`.
                        "z" if ke.shift_key() => {
                            perform_redo(&mut r, &sheets, &active);
                        }
                        "z" => {
                            perform_undo(&mut r, &sheets, &active);
                        }
                        "y" => {
                            perform_redo(&mut r, &sheets, &active);
                        }
                        // Ctrl+- (and Ctrl+NumpadSubtract) → delete dialog
                        // (#14). Whole-row or whole-col selection runs the
                        // operation directly; partial selection shows the
                        // dialog.
                        "-" | "Subtract" | "NumpadSubtract" => {
                            let sel = r.selection_bounds();
                            let is_full_row =
                                sel.1 == 0
                                    && sel.3 as usize
                                        == r.data.col_count().saturating_sub(1);
                            let is_full_col =
                                sel.0 == 0
                                    && sel.2 as usize
                                        == r.data.row_count().saturating_sub(1);
                            if is_full_row {
                                r.delete_rows_at_selection();
                            } else if is_full_col {
                                r.delete_cols_at_selection();
                            } else {
                                show_delete_dialog = true;
                                handled = false;
                            }
                        }
                        "home" => r.select_and_reveal(0, 0),
                        "end" => {
                            let (mr, mc) = r.data.used_extent().unwrap_or((0, 0));
                            r.select_and_reveal(mr, mc);
                        }
                        _ => handled = false,
                    }
                    if handled {
                        r.render();
                    }
                }
                if handled {
                    ke.prevent_default();
                    sync();
                } else if show_delete_dialog {
                    ke.prevent_default();
                    if let Some(open) = delete_open.borrow().as_ref() {
                        open();
                    }
                }
                return;
            }

            // Typing a printable key starts edit mode for the active cell
            // (Excel / Google Sheets convention). The editor's textarea is
            // focused + its content selected inside `start_edit`, so the
            // browser's native key handling then inserts the typed
            // character, replacing the cell content.
            if is_typing_to_edit_key(&key, ke.ctrl_key(), ke.meta_key(), ke.alt_key()) {
                // Don't steal focus from another input-like element (find
                // & replace textarea, data-validation select, etc.).
                let focus_steal_ok = document()
                    .active_element()
                    .map(|el| {
                        let tag = el.tag_name();
                        !tag.eq_ignore_ascii_case("input")
                            && !tag.eq_ignore_ascii_case("textarea")
                            && !tag.eq_ignore_ascii_case("select")
                    })
                    .unwrap_or(true);
                if focus_steal_ok {
                    let (ri, ci) = {
                        let r = renderer.borrow();
                        let s = r.get_selector();
                        (s.ri, s.ci)
                    };
                    let (ri, ci) = renderer.borrow().merge_origin(ri, ci);
                    start_edit(
                        &renderer,
                        &textarea,
                        editor_error.as_ref(),
                        &editing,
                        ri,
                        ci,
                    );
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
                // Tab moves right, Shift+Tab left (#41).
                "Tab" => dc = if ke.shift_key() { -1 } else { 1 },
                "PageDown" => dr = renderer.borrow().rows_per_page() as i32,
                "PageUp" => dr = -(renderer.borrow().rows_per_page() as i32),
                // Home → start of row; End → last filled cell in the row (#41).
                "Home" | "End" => {
                    let (ri, target) = {
                        let r = renderer.borrow();
                        let s = r.get_selector();
                        let col = if key == "Home" {
                            0
                        } else {
                            r.data.row_last_filled_col(s.ri)
                        };
                        (s.ri, col)
                    };
                    {
                        let mut r = renderer.borrow_mut();
                        r.select_and_reveal(ri, target);
                        r.render();
                    }
                    ke.prevent_default();
                    sync();
                    return;
                }
                "Enter" | "F2" => {
                    let (ri, ci) = {
                        let r = renderer.borrow();
                        let s = r.get_selector();
                        (s.ri, s.ci)
                    };
                    start_edit(
                        &renderer,
                        &textarea,
                        editor_error.as_ref(),
                        &editing,
                        ri,
                        ci,
                    );
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
                    if commit_edit(&renderer, &textarea_inner, editor_error.as_ref(), &editing)
                        .is_ok()
                    {
                        let mut r = renderer.borrow_mut();
                        r.move_selection(1, 0);
                        r.render();
                    }
                    sync();
                }
                "Tab" => {
                    ke.prevent_default();
                    ke.stop_propagation();
                    if commit_edit(&renderer, &textarea_inner, editor_error.as_ref(), &editing)
                        .is_ok()
                    {
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
            // `dragging` is only set by a plain cell-selection gesture (resize/
            // scrollbar/fill drags return early at mousedown), so this is the
            // moment a selection finalizes — where an armed Format Painter
            // applies to the freshly-selected range (issue #31).
            let was_selecting = *dragging.borrow();
            if was_fill {
                {
                    let mut r = renderer.borrow_mut();
                    r.apply_fill();
                    r.render();
                }
                sync();
            } else if was_selecting && renderer.borrow().is_format_painter_armed() {
                {
                    let mut r = renderer.borrow_mut();
                    r.apply_format_painter();
                    r.render();
                }
                super::util::set_canvas_cursor(None);
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

/// Ctrl+Z / undo (issue #62). The renderer's unified `undo_stack` is
/// already a `Vec<WorkbookSnapshot>`; the renderer takes the registry
/// and active-sheet parameters and restores everything in one step.
/// No more "is this a pivot?" branching at the call site.
fn perform_undo(
    renderer: &mut std::cell::RefMut<TableRenderer>,
    sheets: &SheetsRegistry,
    active: &ActiveSheet,
) {
    renderer.undo(sheets, active);
}

/// Ctrl+Y / redo (issue #62). Same shape as `perform_undo`; the
/// renderer mirrors the snapshot back into `sheets` and `active`.
fn perform_redo(
    renderer: &mut std::cell::RefMut<TableRenderer>,
    sheets: &SheetsRegistry,
    active: &ActiveSheet,
) {
    renderer.redo(sheets, active);
}

/// True when a window-level keydown should start edit mode for the
/// currently selected cell — i.e. the user typed a printable character
/// without a Ctrl/Meta/Alt modifier. Length-1 keys cover letters,
/// digits, space, and punctuation; multi-char names like `"Enter"`,
/// `"Tab"`, `"Backspace"`, `"F2"`, `"ArrowDown"`, `"Process"`, and
/// `"Dead"` are excluded.
fn is_typing_to_edit_key(key: &str, ctrl: bool, meta: bool, alt: bool) -> bool {
    if ctrl || meta || alt {
        return false;
    }
    key.chars().count() == 1
}

#[cfg(test)]
mod tests {
    use super::is_typing_to_edit_key;

    #[test]
    fn letter_triggers_edit() {
        assert!(is_typing_to_edit_key("a", false, false, false));
        assert!(is_typing_to_edit_key("Z", false, false, false));
    }

    #[test]
    fn digit_triggers_edit() {
        assert!(is_typing_to_edit_key("0", false, false, false));
        assert!(is_typing_to_edit_key("9", false, false, false));
    }

    #[test]
    fn space_and_punctuation_trigger_edit() {
        assert!(is_typing_to_edit_key(" ", false, false, false));
        assert!(is_typing_to_edit_key("=", false, false, false));
        assert!(is_typing_to_edit_key(",", false, false, false));
        assert!(is_typing_to_edit_key("/", false, false, false));
    }

    #[test]
    fn navigation_keys_do_not_trigger_edit() {
        assert!(!is_typing_to_edit_key("Enter", false, false, false));
        assert!(!is_typing_to_edit_key("Tab", false, false, false));
        assert!(!is_typing_to_edit_key("Backspace", false, false, false));
        assert!(!is_typing_to_edit_key("Escape", false, false, false));
        assert!(!is_typing_to_edit_key("Delete", false, false, false));
    }

    #[test]
    fn arrow_keys_do_not_trigger_edit() {
        assert!(!is_typing_to_edit_key("ArrowUp", false, false, false));
        assert!(!is_typing_to_edit_key("ArrowDown", false, false, false));
        assert!(!is_typing_to_edit_key("ArrowLeft", false, false, false));
        assert!(!is_typing_to_edit_key("ArrowRight", false, false, false));
    }

    #[test]
    fn function_keys_do_not_trigger_edit() {
        assert!(!is_typing_to_edit_key("F2", false, false, false));
        assert!(!is_typing_to_edit_key("F1", false, false, false));
        assert!(!is_typing_to_edit_key("PageUp", false, false, false));
        assert!(!is_typing_to_edit_key("PageDown", false, false, false));
    }

    #[test]
    fn ime_composition_keys_do_not_trigger_edit() {
        assert!(!is_typing_to_edit_key("Process", false, false, false));
        assert!(!is_typing_to_edit_key("Dead", false, false, false));
        assert!(!is_typing_to_edit_key("Unidentified", false, false, false));
    }

    #[test]
    fn ctrl_modifier_disables_edit() {
        assert!(!is_typing_to_edit_key("a", true, false, false));
        assert!(!is_typing_to_edit_key("v", true, false, false));
        assert!(!is_typing_to_edit_key("z", true, false, false));
    }

    #[test]
    fn meta_modifier_disables_edit() {
        assert!(!is_typing_to_edit_key("a", false, true, false));
        assert!(!is_typing_to_edit_key("v", false, true, false));
    }

    #[test]
    fn alt_modifier_disables_edit() {
        assert!(!is_typing_to_edit_key("a", false, false, true));
    }

    #[test]
    fn empty_key_does_not_trigger_edit() {
        assert!(!is_typing_to_edit_key("", false, false, false));
    }
}
