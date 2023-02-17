mod core;
mod component;
mod canvas;

use std::f64;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use crate::core::cell::Cell;
use crate::core::evaluable::Evaluable;
use component::element::Element;

use web_sys::HtmlCanvasElement;
use web_sys::CanvasRenderingContext2d;
use canvas::draw::Draw;

#[wasm_bindgen]
pub struct Foo {
    internal: i32,
}

#[wasm_bindgen]
impl Foo {
    #[wasm_bindgen(constructor)]
    pub fn new(val: i32) -> Foo {
        Foo { internal: val }
    }

    pub fn get(&self) -> i32 {
        self.internal
    }

    pub fn set(&mut self, val: i32) {
        self.internal = val;
    }
}

#[wasm_bindgen]
extern {
    pub fn alert(s: String);
}

#[wasm_bindgen(start)]
pub fn start() {

    let cell = Cell {
        text: String::from("abc")
    };

    print!("cell text: {}", cell.evaluate());

    let document = web_sys::window().unwrap().document().unwrap();
    let canvas = document.get_element_by_id("zedsheet").unwrap();
    let canvas: HtmlCanvasElement = canvas
        .dyn_into::<HtmlCanvasElement>()
        .map_err(|_| ())
        .unwrap();

    let context = canvas
        .get_context("2d")
        .unwrap()
        .unwrap()
        .dyn_into::<CanvasRenderingContext2d>()
        .unwrap();

    let draw = Draw {el: canvas, ctx: context};

    draw.resize(500u32, 500u32);

    draw.fill_rect(100u32, 100u32, 100u32, 100u32);



    let targetEl = document.query_selector("#zedsheet").unwrap().unwrap();
    let div = Element::new("div".to_string(), "red".to_string());
    targetEl.append_child(&div.el);
    // context.begin_path();

    // // Draw the outer circle.
    // context
    //     .arc(75.0, 75.0, 50.0, 0.0, f64::consts::PI * 2.0)
    //     .unwrap();

    // // Draw the mouth.
    // context.move_to(110.0, 75.0);
    // context.arc(75.0, 75.0, 35.0, 0.0, f64::consts::PI).unwrap();

    // // Draw the left eye.
    // context.move_to(65.0, 65.0);
    // context
    //     .arc(60.0, 65.0, 5.0, 0.0, f64::consts::PI * 2.0)
    //     .unwrap();

    // // Draw the right eye.
    // context.move_to(95.0, 65.0);
    // context
    //     .arc(90.0, 65.0, 5.0, 0.0, f64::consts::PI * 2.0)
    //     .unwrap();

    // context.stroke();
    // let tag_name = String::from("p");
    // ElementEx::new(tag_name);
}