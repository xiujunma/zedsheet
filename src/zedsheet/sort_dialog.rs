//! Sort dialog (Data → Sort): multi-level sort keys with "has header row"
//! toggle. Mirrors the chart/delete modal pattern.

#[allow(unused_imports)]
use super::*;
use crate::component::element::Element;
use crate::core::auto_filter::Sort;
use crate::renderer::alphabets::xy2expr;
use wasm_bindgen::JsCast;
use web_sys::{HtmlElement, HtmlInputElement, HtmlSelectElement};

pub(crate) fn sort_dialog_html() -> String {
    let row = "display:flex;align-items:center;gap:8px;margin-bottom:6px;";
    let label = "width:60px;flex:none;font-size:12px;";
    format!(
        r##"<div class="zedsheet-modal zs-sort-root" role="dialog" aria-modal="true" style="display:none;position:fixed;left:50%;top:50%;transform:translate(-50%,-50%);z-index:1100;background:#fff;border-radius:4px;border:1px solid rgba(0,0,0,0.1);box-shadow:rgba(0,0,0,0.2) 0px 2px 8px;font-size:13px;line-height:1.25em;width:380px;">
            <div class="zedsheet-modal-header" style="padding:8px 12px;border-bottom:1px solid #e6e6e6;font-weight:600;display:flex;align-items:center;justify-content:space-between;">
                <span>Sort</span>
                <span class="zs-sort-close" role="button" tabindex="0" aria-label="Close" style="cursor:pointer;color:#999;font-size:14px;">✕</span>
            </div>
            <div class="zedsheet-modal-content" style="padding:12px;">
                <div style="{row}margin-bottom:10px;">
                    <label style="{label}"></label>
                    <label style="width:90px;flex:none;font-size:11px;color:#666;">Column</label>
                    <label style="width:90px;flex:none;font-size:11px;color:#666;">Order</label>
                </div>
                <div class="zs-sort-keys">
                    <div class="zs-sort-key" data-sk="0" style="{row}">
                        <span style="{label}font-weight:600;">Sort by</span>
                        <input class="zs-sort-col0" style="width:90px;padding:3px;" placeholder="A"/>
                        <select class="zs-sort-order0" style="width:90px;padding:3px;">
                            <option value="asc">A → Z</option>
                            <option value="desc">Z → A</option>
                        </select>
                        <span class="zs-sort-del" data-sk="0" style="cursor:pointer;color:#999;visibility:hidden;">✕</span>
                    </div>
                    <div class="zs-sort-key" data-sk="1" style="{row}">
                        <span style="{label}font-weight:600;">Then by</span>
                        <input class="zs-sort-col1" style="width:90px;padding:3;" placeholder="(optional)"/>
                        <select class="zs-sort-order1" style="width:90px;padding:3;">
                            <option value="asc">A → Z</option>
                            <option value="desc">Z → A</option>
                        </select>
                        <span class="zs-sort-del" data-sk="1" style="cursor:pointer;color:#999;visibility:hidden;">✕</span>
                    </div>
                    <div class="zs-sort-key" data-sk="2" style="{row}">
                        <span style="{label}font-weight:600;">Then by</span>
                        <input class="zs-sort-col2" style="width:90px;padding:3;" placeholder="(optional)"/>
                        <select class="zs-sort-order2" style="width:90px;padding:3;">
                            <option value="asc">A → Z</option>
                            <option value="desc">Z → A</option>
                        </select>
                        <span class="zs-sort-del" data-sk="2" style="cursor:pointer;color:#999;visibility:hidden;">✕</span>
                    </div>
                </div>
                <div style="{row}margin-top:6px;">
                    <input type="checkbox" class="zs-sort-has-header" checked style="margin:0;"/>
                    <label style="font-size:12px;">Data has header row</label>
                </div>
                <div style="display:flex;justify-content:flex-end;gap:8px;margin-top:10px;">
                    <button class="zs-sort-apply" style="padding:4px 12px;cursor:pointer;">Sort</button>
                    <button class="zs-sort-cancel" style="padding:4px 12px;cursor:pointer;">Cancel</button>
                </div>
            </div>
        </div>"##,
        row = row,
        label = label,
    )
}

fn hide(modal: &web_sys::Element) {
    let _ = modal
        .unchecked_ref::<HtmlElement>()
        .style()
        .set_property("display", "none");
}

pub(crate) fn open_sort_dialog(modal: &web_sys::Element, renderer: &SharedRenderer) {
    // Prefill the first sort column from the active cell.
    let r = renderer.borrow();
    let sel = r.get_selector();
    let col_ref = xy2expr(sel.ci, 0);
    let set = |sel: &str, v: &str| {
        if let Ok(Some(e)) = modal.query_selector(sel) {
            if let Ok(i) = e.dyn_into::<HtmlInputElement>() {
                i.set_value(v);
            }
        }
    };
    set(".zs-sort-col0", &col_ref);
    // Clear other entries.
    for i in 1..3 {
        set(&format!(".zs-sort-col{}", i), "");
        if let Ok(Some(s)) = modal.query_selector(&format!(".zs-sort-order{}", i)) {
            if let Ok(sel) = s.dyn_into::<HtmlSelectElement>() {
                sel.set_value("asc");
            }
        }
    }
    let _ = modal
        .unchecked_ref::<HtmlElement>()
        .style()
        .set_property("display", "block");
}

pub(crate) fn wire_sort_dialog(modal: web_sys::Element, renderer: &SharedRenderer, sync: &SyncFn) {
    let renderer = renderer.clone();
    let sync = sync.clone();
    let modal_node = modal.clone();
    let mut el: Element = modal.into();
    el.add_event_listener("click", move |event: web_sys::Event| {
        let Some(target) = event.target() else {
            return;
        };
        let Ok(elx) = target.dyn_into::<web_sys::Element>() else {
            return;
        };

        if elx
            .closest(".zs-sort-close, .zs-sort-cancel")
            .ok()
            .flatten()
            .is_some()
        {
            hide(&modal_node);
            return;
        }

        if elx.closest(".zs-sort-apply").ok().flatten().is_some() {
            let has_header = modal_node
                .query_selector(".zs-sort-has-header")
                .ok()
                .flatten()
                .and_then(|e| e.dyn_into::<HtmlInputElement>().ok())
                .map(|cb| cb.checked())
                .unwrap_or(true);
            let mut sorts: Vec<Sort> = Vec::new();
            for i in 0..3 {
                let col: Option<String> = modal_node
                    .query_selector(&format!(".zs-sort-col{}", i))
                    .ok()
                    .flatten()
                    .and_then(|e| e.dyn_into::<HtmlInputElement>().ok())
                    .map(|inp| inp.value().trim().to_string());
                let order: Option<String> = modal_node
                    .query_selector(&format!(".zs-sort-order{}", i))
                    .ok()
                    .flatten()
                    .and_then(|e| e.dyn_into::<HtmlSelectElement>().ok())
                    .map(|s| s.value());
                if let (Some(col), Some(order)) = (col, order) {
                    if !col.is_empty() && crate::formula::parser::looks_like_cell_ref(&col) {
                        // Shape-valid but usize-overflowing rows decode to None.
                        if let Some((ci, _)) = crate::renderer::alphabets::exp2xy(&col) {
                            sorts.push(Sort::new(ci, &order));
                        }
                    }
                }
            }
            if sorts.is_empty() {
                hide(&modal_node);
                return;
            }
            {
                let mut r = renderer.borrow_mut();
                if has_header && r.data.auto_filter.active() {
                    r.sort_filter_multi(&sorts);
                } else if !has_header {
                    // Sort the entire used extent (full-sheet sort).
                    // Ensure an autofilter range exists covering the data.
                    if let Some((mr, mc)) = r.data.used_extent() {
                        r.data.auto_filter.ref_ = Some(
                            crate::core::cell_range::CellRange::new(
                                if has_header { 0 } else { 1 },
                                0,
                                mr,
                                mc,
                            )
                            .to_string(),
                        );
                        if !has_header {
                            // Set the sort directly, then sort the entire range
                            // without skipping a header row. We need a raw sort
                            // on the used extent.
                            r.data.sort_filter_range_multi(&sorts);
                        } else {
                            r.sort_filter_multi(&sorts);
                        }
                    }
                } else {
                    // No autofilter active: set one up over the used extent
                    if let Some((mr, mc)) = r.data.used_extent() {
                        r.data.auto_filter.ref_ =
                            Some(crate::core::cell_range::CellRange::new(0, 0, mr, mc).to_string());
                        r.sort_filter_multi(&sorts);
                    }
                }
                r.render();
            }
            sync();
            hide(&modal_node);
        }
    });
}
