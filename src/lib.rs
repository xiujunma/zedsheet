use component::options::Options;
use wasm_bindgen::prelude::*;
use zedsheet::ZedSheet;

mod renderer;
mod component;
mod zedsheet;
mod config;

#[wasm_bindgen(start)]
pub fn start() {
    ZedSheet::new("#zedsheet", Options::default());
}