use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::HtmlCanvasElement;

mod renderer;
mod component;
mod zedsheet;

#[wasm_bindgen(start)]
pub fn start() {
    let document = web_sys::window().unwrap().document().unwrap();
    let element = document.get_element_by_id("zedsheet").unwrap();
    let canvas: HtmlCanvasElement = element
        .dyn_into::<HtmlCanvasElement>()
        .map_err(|_| ())
        .unwrap();
}