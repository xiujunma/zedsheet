#![allow(dead_code)]

use crate::renderer::table_renderer::TableRenderer;
use crate::renderer::area::Area;

pub struct Viewport {
    pub areas: Vec<Area>,
    pub header_areas: Vec<Area>,
}

impl Viewport {
    pub fn new(render: &TableRenderer) -> Viewport {
        let (tx, ty) = (render.row_header.height, render.col_header.width);
        let (frow, fcol) = render.freeze;
        let start_row = render.start_row;
        let start_col = render.start_col;
        let rows = render.rows;
        let cols = render.cols;
        let width = render.width;
        let height = render.height;

        return Viewport {
            areas: Vec::new(),
            header_areas: Vec::new(),
        };
    }
}