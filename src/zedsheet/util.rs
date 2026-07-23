//! Small DOM/formatting helpers shared across the zedsheet UI submodules.

use gloo::utils::document;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::HtmlElement;

use crate::component::element::Element;
use crate::config::CSS_PREFIX;
use crate::renderer::alphabets::exp2xy;

/// Set (or clear, with `None`) the grid canvas's CSS cursor. Used by Format
/// Painter to signal its armed state (issue #31). Targets the first mounted
/// grid canvas.
pub(crate) fn set_canvas_cursor(cursor: Option<&str>) {
    let Some(canvas) = document()
        .query_selector("canvas.zedsheet-table")
        .ok()
        .flatten()
    else {
        return;
    };
    let Some(he) = canvas.dyn_ref::<HtmlElement>() else {
        return;
    };
    match cursor {
        Some(c) => {
            let _ = he.style().set_property("cursor", c);
        }
        None => {
            let _ = he.style().remove_property("cursor");
        }
    }
}

pub(crate) fn client_box(el: &Element) -> (f64, f64) {
    el.el
        .as_ref()
        .and_then(|e| {
            e.dyn_ref::<web_sys::HtmlElement>()
                .map(|h| (h.client_width() as f64, h.client_height() as f64))
        })
        .unwrap_or((0f64, 0f64))
}

/// Toggle the `active` class on a toolbar button identified by its data-action.
pub(crate) fn toggle_active(toolbar: &web_sys::Element, action: &str, on: bool) {
    if let Ok(Some(btn)) = toolbar.query_selector(&format!("[data-action=\"{}\"]", action)) {
        let cl = btn.class_list();
        if on {
            let _ = cl.add_1("active");
        } else {
            let _ = cl.remove_1("active");
        }
    }
}

/// Toggle the `disabled` class on a toolbar button identified by its data-action.
pub(crate) fn toggle_disabled(toolbar: &web_sys::Element, action: &str, on: bool) {
    if let Ok(Some(btn)) = toolbar.query_selector(&format!("[data-action=\"{}\"]", action)) {
        let cl = btn.class_list();
        if on {
            let _ = cl.add_1("disabled");
        } else {
            let _ = cl.remove_1("disabled");
        }
    }
}

/// Set the text content of an element by id (used for dropdown titles).
pub(crate) fn set_text_by_id(id: &str, text: &str) {
    if let Some(el) = document().get_element_by_id(id) {
        el.set_text_content(Some(text));
    }
}

/// Swap the sprite icon on a toolbar icon button (its inner `-icon-img`), so a
/// dropdown button can reflect the active cell's value — e.g. the align
/// dropdowns show the current alignment and update when the selection changes
/// (matching x-spreadsheet's DropdownItem).
pub(crate) fn set_toolbar_icon(toolbar: &web_sys::Element, action: &str, icon: &str) {
    let selector = format!("[data-action=\"{}\"] .{}-icon-img", action, CSS_PREFIX);
    if let Ok(Some(img)) = toolbar.query_selector(&selector) {
        let _ = img.set_attribute("class", &format!("{}-icon-img {}", CSS_PREFIX, icon));
    }
}

/// Short name for a format key, shown in the toolbar's format-dropdown
/// title (a 72px box — longer "name + sample" labels wrap and bleed into
/// the formula bar). The dropdown MENU items carry the full name + sample
/// text; this title shows the name only.
pub(crate) fn format_label(key: &str) -> &'static str {
    match key {
        "number" => "Number",
        "percent" => "Percent",
        "rmb" => "RMB",
        "usd" => "USD",
        "eur" => "EUR",
        "date" => "Date",
        "time" => "Time",
        "datetime" => "Date Time",
        "text" => "Text",
        _ => "Normal",
    }
}

/// Parse a single cell reference like `B3` to `(col, row)` (0-based), returning
/// None for anything that isn't `letters+digits` (so `exp2xy` can't panic).
pub(crate) fn parse_ref(s: &str) -> Option<(usize, usize)> {
    let s = s.trim();
    let mut seen_digit = false;
    let (mut has_letter, mut has_digit) = (false, false);
    for c in s.chars() {
        if c.is_ascii_alphabetic() {
            if seen_digit {
                return None; // letters must precede digits
            }
            has_letter = true;
        } else if c.is_ascii_digit() {
            seen_digit = true;
            has_digit = true;
        } else {
            return None;
        }
    }
    if has_letter && has_digit {
        // Shape is valid, but a 20-digit row still overflows usize.
        exp2xy(s)
    } else {
        None
    }
}

/// True if `s` is acceptable as a named-range name: it starts with a letter and
/// is otherwise letters / digits / underscore. (Strings that parse as a cell
/// reference are handled as references before this is reached.)
pub(crate) fn is_valid_name(s: &str) -> bool {
    let s = s.trim();
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() => {}
        _ => return false,
    }
    s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Show a brief toast at the top of the page. Used for validation errors
/// from the formula bar and other commit paths that can't keep the editor
/// open (issue #9). Auto-hides after 2.5 seconds.
pub(crate) fn show_toast(toast: Option<&HtmlElement>, msg: &str) {
    if let Some(t) = toast {
        t.set_text_content(Some(msg));
        let _ = t.style().set_property("display", "block");
        let toast_for_hide = t.clone();
        let cb = Closure::<dyn FnMut()>::new(move || {
            let _ = toast_for_hide.style().set_property("display", "none");
        });
        if let Some(w) = web_sys::window() {
            let _ = w.set_timeout_with_callback_and_timeout_and_arguments_0(
                cb.as_ref().unchecked_ref(),
                2500,
            );
        }
        cb.forget();
    }
}

/// Tell the user a non-contiguous (Ctrl+click) selection can't be copied
/// (issue #19/H6), reusing the app-level toast element.
pub(crate) fn noncontiguous_copy_toast() {
    let toast = document()
        .query_selector(".zs-dv-toast")
        .ok()
        .flatten()
        .and_then(|e| e.dyn_into::<HtmlElement>().ok());
    show_toast(
        toast.as_ref(),
        "Can't copy a non-contiguous selection — select a single range.",
    );
}

pub(crate) fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Show the color palette beneath a toolbar button.
pub(crate) fn show_palette_under(palette: &web_sys::Element, button: &web_sys::Element) {
    let rect = button.get_bounding_client_rect();
    let style = palette.unchecked_ref::<web_sys::HtmlElement>().style();
    let _ = style.set_property("left", &format!("{}px", rect.left()));
    let _ = style.set_property("top", &format!("{}px", rect.bottom()));
    let _ = style.set_property("display", "block");
}

pub(crate) fn hide_palette(palette: &web_sys::Element) {
    let _ = palette
        .unchecked_ref::<web_sys::HtmlElement>()
        .style()
        .set_property("display", "none");
}

pub(crate) fn hide_tooltip(tooltip: &web_sys::Element) {
    let _ = tooltip
        .unchecked_ref::<web_sys::HtmlElement>()
        .style()
        .set_property("display", "none");
}

// ---------------------------------------------------------------------
// Slicer panel drag/resize geometry (Phase 1.1)
//
// The mousemove handler reads these on every tick; they're pure so
// they're unit-testable without a browser.
// ---------------------------------------------------------------------

/// Minimum panel dimensions in CSS pixels. Below these the chips
/// start clipping or the close button overlaps the title.
pub(crate) const MIN_SLICER_PANEL_W: f64 = 140.0;
pub(crate) const MIN_SLICER_PANEL_H: f64 = 60.0;

/// New panel position from a drag-start snapshot and the current pointer
/// position. No clamping — the user can park a panel anywhere (Excel
/// behaviour, including negative coordinates that scroll out of view).
pub(crate) fn slicer_drag_position(
    start_panel_x: f64,
    start_panel_y: f64,
    start_pointer_x: f64,
    start_pointer_y: f64,
    cur_x: f64,
    cur_y: f64,
) -> (f64, f64) {
    (
        start_panel_x + (cur_x - start_pointer_x),
        start_panel_y + (cur_y - start_pointer_y),
    )
}

/// New panel size from a drag-start snapshot and the current pointer
/// position, clamped to the per-axis minimums. No maximum — overflow
/// past the viewport is allowed.
#[allow(clippy::too_many_arguments)]
pub(crate) fn slicer_resize_size(
    start_panel_w: f64,
    start_panel_h: f64,
    start_pointer_x: f64,
    start_pointer_y: f64,
    cur_x: f64,
    cur_y: f64,
    min_w: f64,
    min_h: f64,
) -> (f64, f64) {
    let w = (start_panel_w + (cur_x - start_pointer_x)).max(min_w);
    let h = (start_panel_h + (cur_y - start_pointer_y)).max(min_h);
    (w, h)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drag_translates_panel_by_pointer_delta() {
        // Panel starts at (100, 80); pointer starts at (50, 40).
        // Pointer moves to (60, 55) → panel moves by (+10, +15).
        let (x, y) = slicer_drag_position(100.0, 80.0, 50.0, 40.0, 60.0, 55.0);
        assert_eq!(x, 110.0);
        assert_eq!(y, 95.0);
    }

    #[test]
    fn drag_with_no_movement_returns_start() {
        let (x, y) = slicer_drag_position(100.0, 80.0, 50.0, 40.0, 50.0, 40.0);
        assert_eq!((x, y), (100.0, 80.0));
    }

    #[test]
    fn drag_allows_negative_panel_position() {
        // Dragging the panel far off the top-left shouldn't snap —
        // Excel lets you park it wherever the cursor goes.
        let (x, y) = slicer_drag_position(0.0, 0.0, 500.0, 500.0, -100.0, -200.0);
        assert_eq!(x, -600.0);
        assert_eq!(y, -700.0);
    }

    #[test]
    fn resize_grows_panel_with_pointer_delta() {
        // 200x180 panel, grip drag from (220, 200) to (250, 240)
        // → +30, +40 → 230x220.
        let (w, h) = slicer_resize_size(
            200.0,
            180.0,
            220.0,
            200.0,
            250.0,
            240.0,
            MIN_SLICER_PANEL_W,
            MIN_SLICER_PANEL_H,
        );
        assert_eq!(w, 230.0);
        assert_eq!(h, 220.0);
    }

    #[test]
    fn resize_shrinks_panel_with_negative_delta() {
        let (w, h) = slicer_resize_size(
            200.0,
            180.0,
            220.0,
            200.0,
            180.0,
            160.0,
            MIN_SLICER_PANEL_W,
            MIN_SLICER_PANEL_H,
        );
        assert_eq!(w, 160.0);
        assert_eq!(h, 140.0);
    }

    #[test]
    fn resize_clamps_width_below_minimum() {
        let (w, h) = slicer_resize_size(
            200.0,
            180.0,
            300.0,
            300.0,
            100.0,
            250.0,
            MIN_SLICER_PANEL_W,
            MIN_SLICER_PANEL_H,
        );
        assert_eq!(w, MIN_SLICER_PANEL_W);
        // Height wasn't being resized (delta_y < 0 but still above min).
        assert_eq!(h, 180.0 + (250.0 - 300.0));
    }

    #[test]
    fn resize_clamps_height_below_minimum() {
        let (w, h) = slicer_resize_size(
            200.0,
            180.0,
            300.0,
            300.0,
            350.0,
            100.0,
            MIN_SLICER_PANEL_W,
            MIN_SLICER_PANEL_H,
        );
        assert_eq!(w, 200.0 + (350.0 - 300.0));
        assert_eq!(h, MIN_SLICER_PANEL_H);
    }

    #[test]
    fn resize_clamps_both_below_minimum() {
        let (w, h) = slicer_resize_size(
            200.0,
            180.0,
            300.0,
            300.0,
            0.0,
            0.0,
            MIN_SLICER_PANEL_W,
            MIN_SLICER_PANEL_H,
        );
        assert_eq!(w, MIN_SLICER_PANEL_W);
        assert_eq!(h, MIN_SLICER_PANEL_H);
    }

    #[test]
    fn min_constants_are_sensible() {
        // Pin the published constants so a future tweak forces an
        // explicit test update — these are user-visible.
        assert_eq!(MIN_SLICER_PANEL_W, 140.0);
        assert_eq!(MIN_SLICER_PANEL_H, 60.0);
    }
}
