//! Protect Sheet modal (Phase 1.3).
//!
//! Two-mode dialog mirroring the active sheet's `SheetProtection`
//! state (set in `core::sheet_protection`):
//!
//! - **Unprotected mode** (`!protection.enabled`): checkbox off,
//!   password field labelled "Password (optional)". Apply enables
//!   protection with the given password (or none).
//!
//! - **Protected mode** (`protection.enabled`): checkbox on,
//!   password field labelled "Password (required to unlock)".
//!   Apply only succeeds if the entered password verifies against
//!   the stored hash. Wrong password surfaces an inline error and
//!   keeps the sheet locked.

use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::{HtmlElement, HtmlInputElement};

use super::*;

const ROOT_CLASS: &str = "zs-protect-sheet-root";

/// HTML for the modal. Hidden by default; opened by the toolbar
/// "Protect Sheet" item or programmatically.
pub(crate) fn protect_sheet_modal_html() -> String {
    let row = "display:flex;align-items:center;gap:8px;margin-bottom:8px;";
    let label = "width:130px;flex:none;";
    format!(
        r##"<div class="zedsheet-modal {root}" role="dialog" aria-modal="true" style="display:none;position:fixed;left:50%;top:50%;transform:translate(-50%,-50%);z-index:1100;background:#fff;border-radius:4px;border:1px solid rgba(0,0,0,0.1);box-shadow:rgba(0,0,0,0.2) 0px 2px 8px;font-size:13px;line-height:1.25em;width:380px;">
            <div class="zedsheet-modal-header" style="padding:8px 12px;border-bottom:1px solid #e6e6e6;font-weight:600;display:flex;align-items:center;justify-content:space-between;">
                <span>Protect Sheet</span>
                <span class="zs-protect-close" role="button" tabindex="0" aria-label="Close" style="cursor:pointer;color:#999;font-size:14px;">✕</span>
            </div>
            <div class="zedsheet-modal-content" style="padding:12px;">
                <div style="display:flex;align-items:center;gap:8px;margin-bottom:12px;">
                    <input class="zs-protect-enable" type="checkbox" id="zs-protect-enable" style="margin:0;"/>
                    <label for="zs-protect-enable" style="cursor:pointer;">Protect this sheet from edits</label>
                </div>
                <div style="color:#666;font-size:11px;margin-bottom:8px;">
                    When protected, the sheet is read-only. Anyone with the password
                    can unlock it; without a password, the protection is "soft" — anyone
                    can disable it.
                </div>
                <div style="{row}">
                    <label class="zs-protect-pwlabel" style="{label}">Password (optional)</label>
                    <input class="zs-protect-password" type="password" style="flex:1;padding:3px;" autocomplete="new-password"/>
                </div>
                <div class="zs-protect-error" style="display:none;color:#b71c1c;font-size:11px;margin-bottom:8px;"></div>
                <div style="display:flex;justify-content:flex-end;gap:8px;">
                    <button class="zs-protect-cancel" style="padding:4px 12px;cursor:pointer;">Cancel</button>
                    <button class="zs-protect-apply" style="padding:4px 12px;cursor:pointer;">Apply</button>
                </div>
            </div>
        </div>"##,
        root = ROOT_CLASS,
        row = row,
        label = label,
    )
}

/// Show the modal and seed it from the active sheet's current
/// `SheetProtection` state.
pub(crate) fn open_protect_sheet_modal(
    modal: &web_sys::Element,
    sheets: &Sheets,
    active: &ActiveSheet,
) {
    // Seed from the active sheet.
    let protection = sheets.borrow()[*active.borrow()].protection.clone();
    let enable_check = modal
        .query_selector(".zs-protect-enable")
        .ok()
        .flatten()
        .and_then(|e| e.dyn_into::<HtmlInputElement>().ok());
    if let Some(cb) = enable_check {
        cb.set_checked(protection.enabled);
    }
    let pw_input = modal
        .query_selector(".zs-protect-password")
        .ok()
        .flatten()
        .and_then(|e| e.dyn_into::<HtmlInputElement>().ok());
    if let Some(pw) = pw_input {
        pw.set_value("");
    }
    let label_el = modal.query_selector(".zs-protect-pwlabel").ok().flatten();
    let label_text = if protection.enabled {
        if protection.password_hash.is_some() {
            "Password (required to unlock)"
        } else {
            "Password (leave blank to disable)"
        }
    } else {
        "Password (optional)"
    };
    if let Some(el) = label_el {
        el.set_text_content(Some(label_text));
    }
    // Clear any leftover error from a previous open.
    if let Some(err) = modal.query_selector(".zs-protect-error").ok().flatten() {
        let _ = err.dyn_ref::<HtmlElement>().map(|h| {
            let _ = h.style().set_property("display", "none");
        });
    }
    let _ = modal.dyn_ref::<HtmlElement>().map(|h| {
        let _ = h.style().set_property("display", "block");
    });
}

fn close_protect_sheet_modal(modal: &web_sys::Element) {
    if let Some(h) = modal.dyn_ref::<HtmlElement>() {
        let _ = h.style().set_property("display", "none");
    }
}

fn show_error(modal: &web_sys::Element, msg: &str) {
    if let Some(err) = modal.query_selector(".zs-protect-error").ok().flatten() {
        if let Some(h) = err.dyn_ref::<HtmlElement>() {
            h.set_text_content(Some(msg));
            let _ = h.style().set_property("display", "block");
        }
    }
}

/// Mount the modal HTML into `root` and wire its Apply / Cancel /
/// close-button handlers. The dialog updates the active sheet's
/// `SheetProtection` via `DataProxy::set_protection`, which also
/// mirrors `enabled` onto `read_only`. Snapshots before mutating so
/// the change is undoable (issue #62).
pub(crate) fn wire_protect_sheet_modal(
    modal: web_sys::Element,
    renderer: &SharedRenderer,
    sheets: &Sheets,
    active: &ActiveSheet,
    sync: &SyncFn,
) {
    let modal_for_apply = modal.clone();
    let sheets_for_apply = sheets.clone();
    let active_for_apply = active.clone();
    let renderer_for_apply = renderer.clone();
    let sync_for_apply = sync.clone();
    let apply_cb = Closure::<dyn FnMut()>::new(move || {
        let ai = *active_for_apply.borrow();
        // Snapshot the active sheet before mutation (undo, issue #62).
        {
            let mut r = renderer_for_apply.borrow_mut();
            // `snapshot()` captures only `self.data`; for a mutator
            // that touches only the active sheet this is enough.
            r.snapshot();
        }
        // Read the modal inputs.
        let enable = modal_for_apply
            .query_selector(".zs-protect-enable")
            .ok()
            .flatten()
            .and_then(|e| e.dyn_into::<HtmlInputElement>().ok())
            .map(|i| i.checked())
            .unwrap_or(false);
        let password = modal_for_apply
            .query_selector(".zs-protect-password")
            .ok()
            .flatten()
            .and_then(|e| e.dyn_into::<HtmlInputElement>().ok())
            .map(|i| i.value())
            .unwrap_or_default();
        let sheets_ref = sheets_for_apply.borrow();
        let protection = sheets_ref[ai].protection.clone();
        drop(sheets_ref);

        // If the sheet is currently protected and a password hash is
        // set, the user must re-enter it to disable. (When enabling,
        // the password is a fresh set, no verification needed.)
        if protection.enabled
            && !enable
            && protection.password_hash.is_some()
            && !protection.verify(&password)
        {
            show_error(&modal_for_apply, "Incorrect password.");
            return;
        }
        let password_opt = if password.is_empty() {
            None
        } else {
            Some(password.as_str())
        };
        {
            let mut s = sheets_for_apply.borrow_mut();
            s[ai].set_protection(enable, password_opt);
        }
        close_protect_sheet_modal(&modal_for_apply);
        // Refresh the toolbar / formula-bar state in case the active
        // cell's locked-cell indicator changes.
        sync_for_apply();
    });
    let apply_btn = modal
        .query_selector(".zs-protect-apply")
        .ok()
        .flatten()
        .and_then(|e| e.dyn_into::<HtmlInputElement>().ok());
    if let Some(btn) = apply_btn {
        let cb = apply_cb.as_ref().unchecked_ref();
        let _ = btn.add_event_listener_with_callback("click", cb);
    }
    apply_cb.forget();

    // Cancel / close: just hide the modal.
    let modal_for_close = modal.clone();
    let close_cb = Closure::<dyn FnMut()>::new(move || {
        close_protect_sheet_modal(&modal_for_close);
    });
    for sel in [".zs-protect-cancel", ".zs-protect-close"] {
        if let Some(el) = modal.query_selector(sel).ok().flatten() {
            if let Ok(btn) = el.dyn_into::<HtmlInputElement>() {
                let cb = close_cb.as_ref().unchecked_ref();
                let _ = btn.add_event_listener_with_callback("click", cb);
            }
        }
    }
    close_cb.forget();
}
