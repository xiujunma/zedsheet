//! Print support (issue #17): render the active sheet's used extent to a
//! print-friendly HTML document, load it into a hidden iframe, and call the
//! iframe's `window.print()`. The browser's native dialog supplies paper
//! size, orientation, margins, and scaling; `@page`/`break-inside` CSS keeps
//! the table pagination tidy. The document builder is pure and host-tested.

#[allow(unused_imports)]
use super::*;
use crate::core::data_proxy::DataProxy;
use crate::core::html_util::{esc, td_style};
use wasm_bindgen::JsCast;
use web_sys::HtmlIFrameElement;

/// Build a standalone HTML document of the sheet's used extent (or the
/// user-defined print area): a fixed-layout table with the grid's column
/// widths and row heights, merged cells as colspan/rowspan, display-formatted
/// values, per-cell styles, and page-setup CSS (issue #14).
pub(crate) fn build_print_html(sheet: &DataProxy) -> String {
    let ps = &sheet.page_setup;
    let (mtop, mright, mbot, mleft) = ps.margins;

    // Resolve the print range: explicit print_area, or the used extent.
    let (min_r, min_c, max_r, max_c) = if let Some(ref area) = ps.print_area {
        if let Ok(cr) = crate::core::cell_range::CellRange::from_str(area) {
            (cr.sri, cr.sci, cr.eri, cr.eci)
        } else if let Some((mr, mc)) = sheet.used_extent() {
            (0, 0, mr, mc)
        } else {
            return empty_print_html(sheet);
        }
    } else if let Some((mr, mc)) = sheet.used_extent() {
        (0, 0, mr, mc)
    } else {
        return empty_print_html(sheet);
    };

    let mut body = String::new();
    body.push_str("<table><colgroup>");
    for c in min_c..=max_c {
        body.push_str(&format!(
            "<col style=\"width:{}px\"/>",
            sheet.get_col_width(c) as i64
        ));
    }
    body.push_str("</colgroup>");
    for r in min_r..=max_r {
        body.push_str(&format!(
            "<tr style=\"height:{}px\">",
            sheet.get_row_height(r) as i64
        ));
        for c in min_c..=max_c {
            let merge = sheet.cell_merge(r, c);
            if let Some(m) = &merge {
                if (r, c) != (m.sri, m.sci) {
                    continue;
                }
            }
            let mut style = sheet.get_cell_style(r, c);
            sheet.apply_cond_format(r, c, &mut style);
            let span = merge
                .map(|m| {
                    format!(
                        " colspan=\"{}\" rowspan=\"{}\"",
                        m.eci - m.sci + 1,
                        m.eri - m.sri + 1
                    )
                })
                .unwrap_or_default();
            body.push_str(&format!(
                "<td{span} style=\"{}\">{}</td>",
                td_style(&style),
                esc(&sheet.cell_display_value(r, c))
            ));
        }
        body.push_str("</tr>");
    }
    body.push_str("</table>");
    let orientation_css = if ps.orientation == "landscape" {
        " size: landscape;"
    } else {
        ""
    };
    let scale_pct = ps.scale.clamp(10, 400);
    format!(
        "<!DOCTYPE html><html><head><meta charset=\"utf-8\"/><title>{title}</title><style>\
           @page {{ margin-top: {mtop}in; margin-right: {mright}in; \
                    margin-bottom: {mbot}in; margin-left: {mleft}in;{orientation_css} }}\
           body {{ margin: 0; font: 12px Arial, sans-serif; \
                  transform: scale({scale}); transform-origin: top left; \
                  width: {scale}%; }}\
           table {{ border-collapse: collapse; table-layout: fixed; }}\
           td {{ border: 1px solid #c8c8c8; padding: 2px 4px; overflow: hidden; \
                 white-space: nowrap; vertical-align: middle; }}\
           tr {{ break-inside: avoid; }}\
         </style></head><body>{body}</body></html>",
        title = esc(&sheet.name),
        mtop = mtop,
        mright = mright,
        mbot = mbot,
        mleft = mleft,
        orientation_css = orientation_css,
        scale = scale_pct as f64 / 100.0,
        body = body
    )
}

/// Map a paper size name to the CSS `@page { size: … }` dimension keywords.
fn pape_size_css(name: &str) -> &'static str {
    match name {
        "letter" => "",
        "a4" => "",
        "legal" => "",
        "a3" => "",
        _ => "",
    }
}

/// Return an empty HTML document (no table) when the sheet has no content
/// and no explicit print area.
fn empty_print_html(sheet: &DataProxy) -> String {
    let ps = &sheet.page_setup;
    let (mtop, mright, mbot, mleft) = ps.margins;
    let orientation_css = if ps.orientation == "landscape" {
        " size: landscape;"
    } else {
        ""
    };
    format!(
        "<!DOCTYPE html><html><head><meta charset=\"utf-8\"/><title>{title}</title><style>\
           @page {{ margin-top: {mtop}in; margin-right: {mright}in; \
                    margin-bottom: {mbot}in; margin-left: {mleft}in;{orientation_css} }}\
           body {{ margin: 0; font: 12px Arial, sans-serif; }}\
         </style></head><body></body></html>",
        title = esc(&sheet.name),
        mtop = mtop,
        mright = mright,
        mbot = mbot,
        mleft = mleft,
        orientation_css = orientation_css,
    )
}

/// Print the active sheet: load the document into a hidden, reused iframe and
/// invoke its `print()` once loaded (issue #17).
pub(crate) fn open_print(renderer: &SharedRenderer) {
    // Print at 100% regardless of the current view zoom (issue #32): the
    // sizing reads get_col_width/get_row_height, which apply the zoom factor,
    // so stamp 1.0 onto a local clone.
    let mut sheet = renderer.borrow().data.clone();
    sheet.set_zoom(1.0);
    let html = build_print_html(&sheet);
    let doc = gloo::utils::document();
    // Replace any previous print frame so repeated prints stay clean.
    if let Ok(Some(old)) = doc.query_selector("#zs-print-frame") {
        old.remove();
    }
    let Ok(frame) = doc.create_element("iframe") else {
        return;
    };
    let _ = frame.set_attribute("id", "zs-print-frame");
    let _ = frame.set_attribute(
        "style",
        "position:fixed;right:0;bottom:0;width:0;height:0;border:0;",
    );
    let _ = frame.set_attribute("srcdoc", &html);
    let Some(body) = doc.body() else { return };
    let _ = body.append_child(&frame);
    // Print once the srcdoc document has loaded.
    if let Some(iframe) = frame.dyn_ref::<HtmlIFrameElement>() {
        let iframe_for_load = iframe.clone();
        let mut el: crate::component::element::Element = frame.clone().into();
        el.add_event_listener("load", move |_e: web_sys::Event| {
            if let Some(w) = iframe_for_load.content_window() {
                let _ = w.focus();
                let _ = w.print();
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::cell_range::CellRange;
    use crate::core::data_proxy::Style;

    #[test]
    fn print_html_contains_formatted_values_and_escapes() {
        let mut d = DataProxy::new("Report");
        d.set_cell_text(0, 0, "<b>safe</b>");
        d.set_cell_text(0, 1, "2");
        d.set_cell_text(0, 2, "=B1*3");
        let html = build_print_html(&d);
        assert!(html.contains("<title>Report</title>"));
        assert!(
            html.contains("&lt;b&gt;safe&lt;/b&gt;"),
            "values are escaped"
        );
        assert!(!html.contains("<b>safe</b>"));
        assert!(
            html.contains("<td style=\"\">6</td>"),
            "formula prints computed"
        );
    }

    #[test]
    fn print_html_renders_merges_as_spans() {
        let mut d = DataProxy::new("t");
        d.set_cell_text(0, 0, "wide");
        d.set_cell_text(1, 1, "x"); // extends the used extent past the merge
        d.merges.add(CellRange::new(0, 0, 0, 1)); // A1:B1
        let html = build_print_html(&d);
        assert!(html.contains("colspan=\"2\" rowspan=\"1\""));
        // The covered cell (0,1) is skipped: the first row has exactly one <td>.
        let first_row = html.split("<tr").nth(1).unwrap();
        assert_eq!(first_row.matches("<td").count(), 1);
    }

    #[test]
    fn print_html_applies_styles_and_cond_formats() {
        let mut d = DataProxy::new("t");
        d.set_cell_text(0, 0, "200");
        let bold = Style {
            bold: true,
            ..Style::default()
        };
        let idx = d.add_style(bold);
        d.set_cell_style(0, 0, idx);
        d.cond_formats.push(crate::core::cond_format::CondRule {
            range: "A1".into(),
            op: "gt".into(),
            v1: "150".into(),
            v2: String::new(),
            v3: String::new(),
            bgcolor: Some("#ffc7ce".into()),
            color: None,
            bold: false,
        });
        let html = build_print_html(&d);
        assert!(html.contains("font-weight:bold;"));
        assert!(
            html.contains("background:#ffc7ce;"),
            "cond format prints too"
        );
    }

    #[test]
    fn empty_sheet_prints_an_empty_document() {
        let html = build_print_html(&DataProxy::new("Empty"));
        assert!(html.contains("<title>Empty</title>"));
        assert!(!html.contains("<table>"));
    }
}
