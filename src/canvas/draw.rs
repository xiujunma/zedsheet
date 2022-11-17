use web_sys::HtmlCanvasElement;
use web_sys::CanvasRenderingContext2d;
use web_sys::window;

pub struct Draw {
    pub el: HtmlCanvasElement,
    pub ctx: CanvasRenderingContext2d
}

fn dpr() -> f64 {
    let window = window().unwrap();
    window.device_pixel_ratio()
}

fn thinLineWidth() -> f64 {
    dpr() - 0.5
}

fn npx(px: u32) -> u32 {
    ((px as f64) * dpr()) as u32
}

impl Draw {
    pub fn resize(&self, width: u32, height: u32) {
        self.el.set_width(width);
        self.el.set_height(height);
    }

    pub fn clear(&self) -> &Self {
        let width = self.el.width();
        let height = self.el.height();
        self.ctx.clear_rect(0f64, 0f64, width as f64, height as f64);
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
}