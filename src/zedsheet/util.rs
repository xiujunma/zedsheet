//! Small DOM/formatting helpers shared across the zedsheet UI submodules.

use gloo::utils::document;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::HtmlElement;

use crate::component::element::Element;
use crate::renderer::alphabets::exp2xy;

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

/// Human label for a format key, shown in the format dropdown title.
pub(crate) fn format_label(key: &str) -> &'static str {
    match key {
        "number" => "Number 1,000.12",
        "percent" => "Percent 10.12%",
        "rmb" => "RMB ￥10.00",
        "usd" => "USD $10.00",
        "eur" => "EUR €10.00",
        "date" => "Date 2024-01-15",
        "time" => "Time 13:30:00",
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
        Some(exp2xy(s))
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
