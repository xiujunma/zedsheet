use crate::renderer::canvas::Canvas;
use crate::renderer::table_renderer::{Gridline, TableRenderer};

pub fn render_lines(canvas: &Canvas, gridline: Gridline, cb: fn()) {
    if gridline.width > 0 {
        //TODO
        canvas.save().begin_path()
    }
}


pub fn render(renderer: &TableRenderer) {

}