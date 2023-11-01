#![allow(dead_code)]

use crate::renderer::table_renderer::{GetRowHeightColWidth, TableRenderer};
use crate::renderer::area::Area;
use crate::renderer::range::Range;

pub struct Viewport {
    pub areas: Vec<Area>,
    pub header_areas: Vec<Area>,
    pub render: TableRenderer,
}

impl GetRowHeightColWidth for Viewport {
    fn get_row_height(&self, index: usize) -> f64 {
        return self.render.get_row_height(index);
    }

    fn get_col_width(&self, index: usize) -> f64 {
        return self.render.get_col_width(index);
    }
}


impl Viewport {
    pub fn new(render: TableRenderer) -> Viewport {
        let (tx, ty) = (render.row_header.height, render.col_header.width);
        let (frow, fcol) = render.freeze;
        let start_row = render.start_row;
        let start_col = render.start_col;
        let rows = render.rows;
        let cols = render.cols;
        let width = render.width;
        let height = render.height;

        let range = Range {
            start_row, start_col, end_row: frow - 1, end_col: fcol - 1
        };

        return Viewport {
            areas: Vec::new(),
            header_areas: Vec::new(),
            render,
        };
    }
}