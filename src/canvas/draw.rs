#![allow(dead_code)]
#![allow(unused_variables)]
use web_sys::HtmlCanvasElement;
use web_sys::CanvasRenderingContext2d;
use web_sys::window;

use super::draw_box::DrawBox;

pub struct Draw {
    pub el: HtmlCanvasElement,
    pub ctx: CanvasRenderingContext2d
}

pub struct Attribute {
    text_align: String,
    text_baseline: String,
    font: String,
    fill_style: String,
    stroke_style: String
}

fn dpr() -> f64 {
    let window = window().unwrap();
    window.device_pixel_ratio()
}

fn thin_line_width() -> f64 {
    dpr() - 0.5
}

fn npx(px: u32) -> f64 {
    (px as f64) * dpr()
}

impl Draw {
    pub fn resize(&self, width: u32, height: u32) {
        self.el.set_width(width);
        self.el.set_height(height);
    }

    pub fn clear(&self) -> &Self {
        let (width, height) = (self.el.width(), self.el.height());
        self.ctx.clear_rect(0f64, 0f64, width as f64, height as f64);
        self
    }

    pub fn attr(&self, attr: &Attribute) -> &Self {
        self.ctx.set_text_align(attr.text_align.as_str());
        self
    }

    pub fn save(&self) -> &Self {
        self.ctx.save();
        self.ctx.begin_path();
        self
    }

    pub fn restore(&self) -> &Self {
        self.ctx.restore();
        self
    }

    pub fn translate(&self, x: u32, y: u32) -> &Self {
        let r = self.ctx.translate(x as f64, y as f64);
        match r {
            Ok(v) => println!("working with version: {v:?}"),
            Err(e) => println!("error parsing header: {e:?}"),
        }
        self
    }

    pub fn scale(&self, x: f64, y: f64) -> &Self {
        let r = self.ctx.scale(x, y);
        match r {
            Ok(v) => println!("working with version: {v:?}"),
            Err(e) => println!("error parsing header: {e:?}"),
        }
        self
    }

    pub fn clear_rect(&self, x: u32, y: u32, w: u32, h: u32) -> &Self {
        self.ctx.clear_rect(x as f64, y as f64, w as f64, h as f64);
        self
    }

    pub fn fill_rect(&self, x: u32, y: u32, w: u32, h: u32) -> &Self {
        self.ctx.fill_rect(npx(x) - 0.5, npx(y) - 0.5, npx(w) - 0.5, npx(h) - 0.5);
        self
    }

    pub fn fill_text(&self, text: &str, x: u32, y: u32) -> &Self {
        let r = self.ctx.fill_text(text, npx(x), npx(y));
        match r {
            Ok(v) => println!("working with version: {v:?}"),
            Err(e) => println!("error parsing header: {e:?}"),
        }
        self
    }

    pub fn text(&self, text: &str, draw_box: &DrawBox, attr: &Attribute, text_wrap: bool) -> &Self {
        draw_box.textx(attr.text_align.as_str());
        let ctx = &self.ctx;
        ctx.save();
        ctx.begin_path();
        self.attr(attr);
        self
    }
}