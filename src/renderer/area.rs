#![allow(dead_code)]

use std::rc::Rc;
use crate::renderer::table_renderer::AreaCell;
use crate::renderer::viewport::{GetColWidth, GetRowHeight};
use super::range::Range;

pub struct Area {
    pub range: Range,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub row_height: GetRowHeight,
    pub col_width: GetColWidth,
}

impl Area {
    pub fn new(range: Range, x: f64, y: f64, width: f64, height: f64, row_height: GetRowHeight, col_width: GetColWidth) -> Self {
        return Area {
            range,
            x,
            y,
            width,
            height,
            row_height,
            col_width,
        }
    }

    pub fn get_row_height(&self, index: usize) -> f64 {
        return self.get_row_height(index)
    }

    pub fn get_col_width(&self, index: usize) -> f64 {
        return self.get_col_width(index)
    }

    pub fn contains_x(&self, x: usize) -> bool {
        return x >= self.x && x <= self.x + self.width
    }

    pub fn contains_y(&self, y: usize) -> bool {
        return y >= self.y && y <= self.y + self.height
    }
    pub fn contains(&self, x: usize, y: usize) -> bool {
        return self.contains_x(x) && self.contains_y(y)
    }

    pub fn each_row<F>(&self, mut f: F)
    where
        F: FnMut(usize, usize, f64),
    {

    }

    pub fn cell_at(x: usize, y: usize) -> AreaCell {

    }
}