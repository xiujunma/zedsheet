extern crate console_error_panic_hook;
use std::panic;

use wasm_bindgen::prelude::*;

mod renderer;
mod component;
mod zedsheet;
mod config;
mod core;
mod formula;

use core::data_proxy::{DataProxy, Style};
use core::cell_range::CellRange;
use component::options::Options;
use zedsheet::ZedSheet;

use std::sync::atomic::{AtomicBool, Ordering};
static STARTED: AtomicBool = AtomicBool::new(false);

#[wasm_bindgen(start)]
pub fn start() {
    panic::set_hook(Box::new(console_error_panic_hook::hook));

    // Guard against repeated init (trunk can run the start hook more than once).
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }

    let mut data = DataProxy::new("sheet1");

    // Bold, centered, shaded header row.
    let mut header = Style::default();
    header.bold = true;
    header.align = "center".to_string();
    header.bgcolor = Some("#e8eef7".to_string());
    let header_idx = data.add_style(header);

    data.set_cell_text(0, 0, "Name");
    data.set_cell_style(0, 0, header_idx);
    data.set_cell_text(0, 1, "Score");
    data.set_cell_style(0, 1, header_idx);

    data.set_cell_text(1, 0, "Alice");
    data.set_cell_text(1, 1, "100");
    data.set_cell_text(2, 0, "Bob");
    data.set_cell_text(2, 1, "200");

    // A formula cell with a highlight style.
    data.set_cell_text(3, 0, "Total");
    data.set_cell_text(3, 1, "=SUM(B2:B3)");
    let mut hl = Style::default();
    hl.bgcolor = Some("#fff3cd".to_string());
    hl.color = "#9a6700".to_string();
    hl.bold = true;
    let hl_idx = data.add_style(hl);
    data.set_cell_style(3, 1, hl_idx);

    // A currency-formatted column.
    data.set_cell_text(0, 2, "Price");
    data.set_cell_style(0, 2, header_idx);
    let mut usd = Style::default();
    usd.format = "usd".to_string();
    usd.align = "right".to_string();
    let usd_idx = data.add_style(usd);
    data.set_cell_text(1, 2, "1234.5");
    data.set_cell_style(1, 2, usd_idx);
    data.set_cell_text(2, 2, "49.99");
    data.set_cell_style(2, 2, usd_idx);

    // A wrapped long-text cell (row given extra height to show the wrap).
    let mut wrap = Style::default();
    wrap.text_wrap = true;
    wrap.valign = "top".to_string();
    let wrap_idx = data.add_style(wrap);
    data.set_cell_text(5, 0, "This is a long sentence that wraps across multiple lines inside the cell.");
    data.set_cell_style(5, 0, wrap_idx);
    data.set_row_height(5, 60.0);

    // A merged cell spanning D1:E2.
    data.set_cell_text(0, 3, "Merged region");
    let mut mstyle = Style::default();
    mstyle.align = "center".to_string();
    mstyle.valign = "middle".to_string();
    mstyle.bgcolor = Some("#d1e7dd".to_string());
    let m_idx = data.add_style(mstyle);
    data.set_cell_style(0, 3, m_idx);
    data.merges.add(CellRange::new(0, 3, 1, 4));

    // Mount the full spreadsheet shell into the #zedsheet container.
    let _sheet = ZedSheet::new("#zedsheet", Options::default(), data);
    std::mem::forget(_sheet);
}
