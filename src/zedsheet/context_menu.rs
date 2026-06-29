#[allow(unused_imports)]
use super::*;
use crate::component::element::Element;
use crate::config::CSS_PREFIX;
use crate::renderer::table_renderer::PasteMode;
use gloo::utils::window;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::MouseEvent;

pub(crate) fn context_menu_html() -> String {
    let item = |cmd: &str, label: &str| {
        format!(
            "<div class=\"{p}-item\" data-cmenu=\"{cmd}\">{label}</div>",
            p = CSS_PREFIX,
            cmd = cmd,
            label = label
        )
    };
    let divider = format!("<div class=\"{p}-item divider\"></div>", p = CSS_PREFIX);
    [
        item("copy", "Copy"),
        item("cut", "Cut"),
        item("paste", "Paste"),
        // Paste Special (issue #28).
        item("paste-values", "Paste values only"),
        item("paste-formulas", "Paste formulas only"),
        item("paste-formats", "Paste formats only"),
        item("paste-transpose", "Paste transposed"),
        item("paste-link", "Paste link"),
        divider.clone(),
        item("insert-row", "Insert row"),
        item("insert-col", "Insert column"),
        item("delete-row", "Delete row"),
        item("delete-col", "Delete column"),
        // Issue #14: cell insert/delete with shift, and hide/unhide.
        item("insert-cells-down", "Insert cells (shift down)"),
        item("insert-cells-right", "Insert cells (shift right)"),
        item("delete-cells-up", "Delete cells (shift up)"),
        item("delete-cells-left", "Delete cells (shift left)"),
        item("hide-rows", "Hide rows"),
        item("hide-cols", "Hide columns"),
        item("unhide-rows", "Unhide rows"),
        item("unhide-cols", "Unhide columns"),
        divider.clone(),
        item("note", "Insert / edit note"),
        item("delete-note", "Delete note"),
        divider.clone(),
        item("link", "Insert / edit link"),
        item("remove-link", "Remove link"),
        divider.clone(),
        item("clear", "Clear contents"),
        // Issue #24: per-cell lock (enforced when the sheet is read-only/protected)
        item("editable", "Lock / unlock cell"),
        // Issue #9: data validation
        item("validation", "Data Validation…"),
        // Issue #11: conditional formatting rules dialog.
        item("condfmt", "Conditional formatting…"),
        // Issue #16: charts dialog.
        item("chart", "Insert chart…"),
        // Issue #35: PivotTable dialog.
        item("pivot", "Insert PivotTable…"),
        // Issue #61: Slicers dialog.
        item("slicer", "Insert Slicer…"),
        // Issue #35: refresh the pivot whose output sheet is the active one.
        item("refresh-pivot", "Refresh pivot"),
        // Issue #30: row/column outline groups + Subtotal.
        divider.clone(),
        item("group-rows", "Group rows"),
        item("ungroup-rows", "Ungroup rows"),
        item("group-cols", "Group columns"),
        item("ungroup-cols", "Ungroup columns"),
        item("subtotal", "Subtotal by first column"),
        // Issue #34: Excel-style tables.
        divider.clone(),
        item("format-table", "Format as table"),
        item("table-totals", "Toggle table total row"),
        item("table-to-range", "Convert table to range"),
        // Text alignment helpers (issue #25). The "set_rotation" /
        // "bump_indent" / "toggle_shrink_to_fit" actions are wired in
        // `wire_context_menu`.
        divider.clone(),
        item("rotate-0", "Rotate 0°"),
        item("rotate-45", "Rotate 45°"),
        item("rotate-90", "Rotate 90°"),
        item("rotate--45", "Rotate -45°"),
        item("shrink-toggle", "Shrink to fit"),
        item("indent-inc", "Increase indent"),
        item("indent-dec", "Decrease indent"),
    ]
    .join("")
}

/// Wire the right-click context menu: open on canvas contextmenu, run the
/// chosen command, and close on outside click.
pub(crate) fn wire_context_menu(
    canvas_el: &mut Element,
    menu_node: web_sys::Element,
    renderer: &SharedRenderer,
    sheets: &Sheets,
    active: &ActiveSheet,
    sync: &SyncFn,
    dv_open: OpenHandle,
    cf_open: OpenHandle,
    chart_open: OpenHandle,
    pivot_open: OpenHandle,
    slicer_open: OpenHandle,
    _delete_open: OpenHandle,
) {
    // Open on right-click, after selecting the cell under the cursor.
    {
        let renderer = renderer.clone();
        let menu = menu_node.clone();
        let sheets = sheets.clone();
        canvas_el.add_event_listener("contextmenu", move |event: web_sys::Event| {
            event.prevent_default();
            let me: MouseEvent = event.dyn_into().unwrap();
            let (x, y) = (me.offset_x() as f64, me.offset_y() as f64);
            let hit = renderer.borrow().cell_at(x, y);
            if let Some((ri, ci)) = hit {
                let mut r = renderer.borrow_mut();
                // Issue #19: only collapse when the right-click is outside
                // every selected range (Excel behavior).
                if !r.contains_selected(ri, ci) {
                    r.clear_multi_range();
                    r.select_cell(ri, ci);
                    r.render();
                }
            }
            let style = menu.unchecked_ref::<web_sys::HtmlElement>().style();
            let _ = style.set_property("display", "block");
            let _ = style.set_property("left", &format!("{}px", x));
            let _ = style.set_property("top", &format!("{}px", y));
            // Hide "Refresh pivot" if the active sheet isn't a pivot output
            // (issue #35). The query is by data-cmenu so the row's
            // display is toggled directly.
            let active_name = renderer.borrow().data.name.clone();
            let any_pivot = sheets
                .borrow()
                .iter()
                .any(|d| d.pivots.iter().any(|p| p.output_sheet == active_name));
            if let Ok(Some(item)) = menu.query_selector("[data-cmenu='refresh-pivot']") {
                let _ = item
                    .unchecked_ref::<web_sys::HtmlElement>()
                    .style()
                    .set_property("display", if any_pivot { "block" } else { "none" });
            }
        });
    }

    // Run a command on item click, then hide.
    {
        let renderer = renderer.clone();
        let sync = sync.clone();
        let menu = menu_node.clone();
        let menu_for_click = menu_node.clone();
        let sheets = sheets.clone();
        let active = active.clone();
        let mut menu_el: Element = menu_node.clone().into();
        menu_el.add_event_listener("click", move |event: web_sys::Event| {
            let Some(target) = event.target() else { return };
            let Ok(el) = target.dyn_into::<web_sys::Element>() else {
                return;
            };
            let cmd = el.get_attribute("data-cmenu").or_else(|| {
                el.closest("[data-cmenu]")
                    .ok()
                    .flatten()
                    .and_then(|e| e.get_attribute("data-cmenu"))
            });
            let Some(cmd) = cmd else { return };

            // Editing a note needs a prompt outside the renderer borrow.
            if cmd == "note" {
                let current = renderer.borrow().selection_note().unwrap_or_default();
                if let Ok(Some(text)) =
                    window().prompt_with_message_and_default("Cell note:", &current)
                {
                    let mut r = renderer.borrow_mut();
                    r.set_selection_note(if text.trim().is_empty() {
                        None
                    } else {
                        Some(text)
                    });
                    r.render();
                }
                let _ = menu_for_click
                    .unchecked_ref::<web_sys::HtmlElement>()
                    .style()
                    .set_property("display", "none");
                return;
            }

            // Editing a hyperlink also needs a prompt outside the renderer borrow.
            if cmd == "link" {
                let current = renderer.borrow().selection_link().unwrap_or_default();
                if let Ok(Some(text)) =
                    window().prompt_with_message_and_default("Link URL:", &current)
                {
                    let mut r = renderer.borrow_mut();
                    // set_selection_link normalizes the URL; blank input clears it.
                    r.set_selection_link(if text.trim().is_empty() {
                        None
                    } else {
                        Some(text)
                    });
                    r.render();
                }
                let _ = menu_for_click
                    .unchecked_ref::<web_sys::HtmlElement>()
                    .style()
                    .set_property("display", "none");
                return;
            }

            // Charts dialog (issue #16): same open-handle pattern.
            if cmd == "chart" {
                if let Some(open) = chart_open.borrow().as_ref() {
                    open();
                }
                let _ = menu_for_click
                    .unchecked_ref::<web_sys::HtmlElement>()
                    .style()
                    .set_property("display", "none");
                return;
            }

            // PivotTable dialog (issue #35): same open-handle pattern.
            if cmd == "pivot" {
                if let Some(open) = pivot_open.borrow().as_ref() {
                    open();
                }
                let _ = menu_for_click
                    .unchecked_ref::<web_sys::HtmlElement>()
                    .style()
                    .set_property("display", "none");
                return;
            }

            // Slicer dialog (issue #61): same open-handle pattern as
            // the chart/pivot modals — the menu just hands control to
            // the open handle set up at mount time.
            if cmd == "slicer" {
                if let Some(open) = slicer_open.borrow().as_ref() {
                    open();
                }
                let _ = menu_for_click
                    .unchecked_ref::<web_sys::HtmlElement>()
                    .style()
                    .set_property("display", "none");
                return;
            }

            // PivotTable refresh (issue #35): re-runs the pivot whose
            // output sheet is the currently active sheet.
            if cmd == "refresh-pivot" {
                let refreshed = {
                    let mut r = renderer.borrow_mut();
                    r.refresh_active_pivot(&sheets, &active)
                };
                if refreshed {
                    sync();
                }
                let _ = menu_for_click
                    .unchecked_ref::<web_sys::HtmlElement>()
                    .style()
                    .set_property("display", "none");
                return;
            }

            // Conditional Formatting dialog (issue #11): same open-handle
            // pattern as the validation modal below.
            if cmd == "condfmt" {
                if let Some(open) = cf_open.borrow().as_ref() {
                    open();
                }
                let _ = menu_for_click
                    .unchecked_ref::<web_sys::HtmlElement>()
                    .style()
                    .set_property("display", "none");
                return;
            }

            // Data Validation modal (issue #9): open before the borrow_mut
            // match below so the open handle can take its own borrow.
            if cmd == "validation" {
                if let Some(open) = dv_open.borrow().as_ref() {
                    open();
                }
                let _ = menu_for_click
                    .unchecked_ref::<web_sys::HtmlElement>()
                    .style()
                    .set_property("display", "none");
                return;
            }

            {
                let mut r = renderer.borrow_mut();
                // Read-only mode blocks every *write* menu action. Copy is
                // read-only on the data, so it stays available (issue #24).
                let read_only = r.data.is_read_only();
                match cmd.as_str() {
                    "copy" => {
                        if !r.copy_selection() {
                            noncontiguous_copy_toast();
                        }
                    }
                    "cut" if !read_only => {
                        if !r.cut_selection() {
                            noncontiguous_copy_toast();
                        }
                    }
                    "paste" if !read_only => r.paste(),
                    "paste-values" if !read_only => r.paste_special(PasteMode::Values),
                    "paste-formulas" if !read_only => r.paste_special(PasteMode::Formulas),
                    "paste-formats" if !read_only => r.paste_special(PasteMode::Formats),
                    "paste-transpose" if !read_only => r.paste_special(PasteMode::Transpose),
                    "paste-link" if !read_only => r.paste_special(PasteMode::Link),
                    "insert-row" if !read_only => r.insert_row_at_selection(),
                    "insert-col" if !read_only => r.insert_col_at_selection(),
                    "delete-row" if !read_only => r.delete_rows_at_selection(),
                    "delete-col" if !read_only => r.delete_cols_at_selection(),
                    // Issue #14: cell insert/delete with shift direction.
                    "insert-cells-down" if !read_only => r.insert_cells_at_selection(false),
                    "insert-cells-right" if !read_only => r.insert_cells_at_selection(true),
                    "delete-cells-up" if !read_only => r.delete_cells_at_selection(false),
                    "delete-cells-left" if !read_only => r.delete_cells_at_selection(true),
                    // Issue #14: hide / unhide rows & columns.
                    "hide-rows" if !read_only => r.hide_rows_at_selection(),
                    "hide-cols" if !read_only => r.hide_cols_at_selection(),
                    "unhide-rows" if !read_only => r.unhide_rows_at_selection(),
                    "unhide-cols" if !read_only => r.unhide_cols_at_selection(),
                    "delete-note" if !read_only => r.set_selection_note(None),
                    "remove-link" if !read_only => r.set_selection_link(None),
                    "clear" if !read_only => r.clear_selection_content(),
                    // Toggle the per-cell `editable` flag on the active cell.
                    // Works regardless of the sheet-wide read-only mode so
                    // a user can mark cells for later protection, but the
                    // toggle itself is a no-op in read-only mode.
                    "editable" if !read_only => r.toggle_selection_editable(),
                    // Text alignment helpers (issue #25). Style changes are
                    // independent of the sheet's read-only mode — they're
                    // presentation, not data, so they apply even on a
                    // locked sheet. The `set_sheets_registry` clone we
                    // update is the renderer's, so the next render uses
                    // the new rotation/indent/shrink_to_fit immediately.
                    "rotate-0" if !read_only => r.set_rotation(0.0),
                    "rotate-45" if !read_only => r.set_rotation(45.0),
                    "rotate-90" if !read_only => r.set_rotation(90.0),
                    "rotate--45" if !read_only => r.set_rotation(-45.0),
                    "shrink-toggle" if !read_only => r.toggle_shrink_to_fit(),
                    "indent-inc" if !read_only => r.bump_indent(10),
                    "indent-dec" if !read_only => r.bump_indent(-10),
                    // Issue #30: outline groups + Subtotal.
                    "group-rows" if !read_only => r.group_rows(),
                    "ungroup-rows" if !read_only => r.ungroup_rows(),
                    "group-cols" if !read_only => r.group_cols(),
                    "ungroup-cols" if !read_only => r.ungroup_cols(),
                    "subtotal" if !read_only => r.subtotal_selection(),
                    // Issue #34: Excel-style tables.
                    "format-table" if !read_only => r.format_selection_as_table(),
                    "table-totals" if !read_only => r.toggle_table_totals_at_selection(),
                    "table-to-range" if !read_only => r.convert_table_at_selection(),
                    _ => {}
                }
                r.render();
            }
            // Refresh the formula bar / undo state and persist the edit (#20).
            sync();
            let _ = menu_for_click
                .unchecked_ref::<web_sys::HtmlElement>()
                .style()
                .set_property("display", "none");
        });
        // Hide when clicking outside the menu.
        let cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
            if let Some(target) = event.target() {
                if let Ok(node) = target.dyn_into::<web_sys::Node>() {
                    if menu.contains(Some(&node)) {
                        return;
                    }
                }
            }
            let _ = menu
                .unchecked_ref::<web_sys::HtmlElement>()
                .style()
                .set_property("display", "none");
        });
        window()
            .add_event_listener_with_callback("mousedown", cb.as_ref().unchecked_ref())
            .unwrap();
        cb.forget();
    }
}
