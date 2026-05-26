extern crate console_error_panic_hook;
use std::panic;

use wasm_bindgen::prelude::*;

mod renderer;
mod component;
mod zedsheet;
mod config;
mod core;
mod formula;

use core::data_proxy::DataProxy;
use component::options::Options;
use zedsheet::ZedSheet;

#[wasm_bindgen(start)]
pub fn start() {
    panic::set_hook(Box::new(console_error_panic_hook::hook));

    let mut data = DataProxy::new("sheet1");

    // Sample data
    data.set_cell_text(0, 0, "Name");
    data.set_cell_text(0, 1, "Score");
    data.set_cell_text(1, 0, "Alice");
    data.set_cell_text(1, 1, "100");
    data.set_cell_text(2, 0, "Bob");
    data.set_cell_text(2, 1, "200");

    // Mount the full spreadsheet shell into the #zedsheet container.
    let _sheet = ZedSheet::new("#zedsheet", Options::default(), data);
    std::mem::forget(_sheet);
}
