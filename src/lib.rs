use component::options::Options;
use wasm_bindgen::prelude::*;
use zedsheet::ZedSheet;

mod renderer;
mod component;
mod zedsheet;
mod config;
mod core;
mod data;

use data::table_data::{ TableData, Table };


const TABLE_DATA: &TableData = &TableData {
    cols: vec![],
    rows: vec![],
    cells: vec![]
};

#[wasm_bindgen(start)]
pub fn start() {
    ZedSheet::new("#zedsheet", Options::default());
}