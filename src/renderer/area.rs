#![allow(dead_code)]

use std::rc::Rc;
use crate::renderer::table_renderer::{AreaCell, Rect};
use crate::renderer::viewport::{GetColWidth, GetRowHeight};
use super::range::Range;

#[derive(Debug, Clone)]
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

    pub fn contains_x(&self, x: f64) -> bool {
        return x >= self.x && x <= self.x + self.width
    }

    pub fn contains_y(&self, y: f64) -> bool {
        return y >= self.y && y <= self.y + self.height
    }
    pub fn contains(&self, x: f64, y: f64) -> bool {
        return self.contains_x(x) && self.contains_y(y)
    }

    pub fn each_row<F>(&self, mut f: F)
    where
        F: FnMut(usize, usize, f64),
    {

    }

    pub fn rect_row(&self, start_row: usize, end_row: usize) -> Rect {
        // TODO
        return Rect {
            x: 0f64,
            y: 0f64,
            width: 0f64,
            height: 0f64,
        }
    }

    pub fn rect_col(&self, start_col: usize, end_col: usize) -> Rect {
        // TODO
        return Rect {
            x: 0f64,
            y: 0f64,
            width: 0f64,
            height: 0f64,
        }
    }

    pub fn rect(&self, r: &Range) -> Rect {
        // TODO
        return Rect {
            x: 0f64,
            y: 0f64,
            width: 0f64,
            height: 0f64,
        }
    }

    pub fn cell_at(&self, x: f64, y: f64) -> AreaCell {
        let start_row = self.range.start_row;
        let start_col = self.range.start_col;

        let mut cell = AreaCell {
            row: start_row,
            col: start_col,
            x: self.x,
            y: self.y,
            width: 0f64,
            height: 0f64,
        };

        // row
        while cell.y < y as f64 {
            let h = self.get_row_height(cell.row);
            cell.row += 1;
            cell.y += h;
            cell.height = h;
        }

        cell.y -= cell.height;
        cell.row -= 1;

        // col
        while cell.x < x as f64 {
            let w = self.get_col_width(cell.col);
            cell.col += 1;
            cell.x += w;
            cell.width = w;
        }

        cell.x -= cell.width;
        cell.col -= 1;

        cell
    }
}