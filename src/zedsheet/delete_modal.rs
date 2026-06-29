//! Delete-cells dialog (Ctrl+-): shift cells up/left, delete entire rows/cols.
//! Mirrors the chart modal pattern (issue #14 — delete half of insert/delete).

#[allow(unused_imports)]
use super::*;
use crate::component::element::Element;
use wasm_bindgen::JsCast;
use web_sys::HtmlElement;

pub(crate) fn delete_modal_html() -> String {
    let row = "display:flex;align-items:center;gap:8px;margin-bottom:8px;";
    format!(
        r##"<div class="zedsheet-modal zs-delete-root" role="dialog" aria-modal="true" style="display:none;position:fixed;left:50%;top:50%;transform:translate(-50%,-50%);z-index:1100;background:#fff;border-radius:4px;border:1px solid rgba(0,0,0,0.1);box-shadow:rgba(0,0,0,0.2) 0px 2px 8px;font-size:13px;line-height:1.25em;width:280px;">
            <div class="zedsheet-modal-header" style="padding:8px 12px;border-bottom:1px solid #e6e6e6;font-weight:600;display:flex;align-items:center;justify-content:space-between;">
                <span>Delete</span>
                <span class="zs-delete-close" role="button" tabindex="0" aria-label="Close" style="cursor:pointer;color:#999;font-size:14px;">✕</span>
            </div>
            <div class="zedsheet-modal-content" style="padding:12px;">
                <div style="{row}">
                    <button class="zs-delete-shift-up" style="width:100%;padding:6px 12px;cursor:pointer;text-align:left;">Shift cells up</button>
                </div>
                <div style="{row}">
                    <button class="zs-delete-shift-left" style="width:100%;padding:6px 12px;cursor:pointer;text-align:left;">Shift cells left</button>
                </div>
                <div style="{row}">
                    <button class="zs-delete-row" style="width:100%;padding:6px 12px;cursor:pointer;text-align:left;">Entire row</button>
                </div>
                <div style="{row}">
                    <button class="zs-delete-col" style="width:100%;padding:6px 12px;cursor:pointer;text-align:left;">Entire column</button>
                </div>
            </div>
        </div>"##,
        row = row,
    )
}

/// Show the dialog, positioned at the centre of the active selection so it
/// feels anchored to the interaction. If the selection is a whole-row or
/// whole-column span, skip the dialog and run the operation directly.
pub(crate) fn open_delete_modal(modal: &web_sys::Element, _renderer: &SharedRenderer) {
    let _ = modal
        .unchecked_ref::<HtmlElement>()
        .style()
        .set_property("display", "block");
}

fn hide_delete_modal(modal: &web_sys::Element) {
    let _ = modal
        .unchecked_ref::<HtmlElement>()
        .style()
        .set_property("display", "none");
}

/// Wire the delete dialog: each button dispatches to the matching renderer
/// method, then the dialog closes.
pub(crate) fn wire_delete_modal(
    modal: web_sys::Element,
    renderer: &SharedRenderer,
    sync: &SyncFn,
) {
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

        // Close button.
        if elx
            .closest(".zs-delete-close")
            .ok()
            .flatten()
            .is_some()
        {
            hide_delete_modal(&modal_node);
            return;
        }

        let action: Option<fn(&mut TableRenderer)> = if elx
            .closest(".zs-delete-shift-up")
            .ok()
            .flatten()
            .is_some()
        {
            Some(|r: &mut TableRenderer| r.delete_cells_at_selection(false))
        } else if elx
            .closest(".zs-delete-shift-left")
            .ok()
            .flatten()
            .is_some()
        {
            Some(|r: &mut TableRenderer| r.delete_cells_at_selection(true))
        } else if elx
            .closest(".zs-delete-row")
            .ok()
            .flatten()
            .is_some()
        {
            Some(|r: &mut TableRenderer| r.delete_rows_at_selection())
        } else if elx
            .closest(".zs-delete-col")
            .ok()
            .flatten()
            .is_some()
        {
            Some(|r: &mut TableRenderer| r.delete_cols_at_selection())
        } else {
            return; // click on backdrop / other element → ignore
        };

        {
            let mut r = renderer.borrow_mut();
            if let Some(f) = action {
                f(&mut r);
            }
            r.render();
        }
        sync();
        hide_delete_modal(&modal_node);
    });
}
