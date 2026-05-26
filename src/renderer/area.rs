#![allow(dead_code)]

use std::collections::HashMap;
use crate::{core::data_proxy::DataProxy, renderer::table_renderer::{AreaCell, Rect}};
use super::range::Range;

#[derive(Clone, Debug)]
pub struct Area {
    // { rowIndex: { y, height }}
    pub row_map: HashMap<usize, (f64, f64)>,
    // { colIndex: { x, width }}
    pub col_map: HashMap<usize, (f64, f64)>,
    pub range: Range,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64
}

impl Area {
    pub fn new(data: DataProxy, range: Range, x: f64, y: f64, width: f64, height: f64) -> Self
    {
        let mut row_map: HashMap<usize, (f64, f64)> = HashMap::new();
        let mut col_map: HashMap<usize, (f64, f64)> = HashMap::new();

        let mut total_height: f64 = 0f64;
        range.each_row(|index| {
            let h = data.get_row_height(index);
            row_map.insert(index, (total_height, h));
            total_height += h;
        }, None);

        let mut height = height;
        if height <= 0f64 {
            height = total_height;
        }

        let mut total_width: f64 = 0f64;
        range.each_col(|index| {
            let w = data.get_col_width(index);
            col_map.insert(index, (total_width, w));
            total_width += w;
        }, None);

        let mut width = width;
        if width <= 0f64 { 
            width = total_width;
        }

        Area {
            row_map,
            col_map,
            range,
            x,
            y,
            width,
            height
        }
    }

    pub fn get_row_height(&self, index: usize) -> f64 {
        let row = self.row_map.get(&index).unwrap();
        return row.1;
    }

    pub fn get_col_width(&self, index: usize) -> f64 {
        let col = self.col_map.get(&index).unwrap();
        return col.1;
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
        F: FnMut(usize, f64, f64),
    {
        self.range.each_row(|row| {
            let (y, height) = self.row_map.get(&row).unwrap();
            f(row, *y, *height);
        }, None);
    }

    pub fn each_col<F>(&self, mut f: F)
    where
        F: FnMut(usize, f64, f64),
    {
        self.range.each_col(|col| {
            let (x, width) = self.col_map.get(&col).unwrap();
            f(col, *x, *width);
        }, None);
    }

    pub fn each<F>(&self, mut f: F)
    where
        F: FnMut(usize, usize, Rect),
    {
        self.each_row(|row, y, height| {
            self.each_col(|col, x, width| {
                f(row, col, Rect { x, y, width, height });
            });
        });
    }

    pub fn rect_row(&self, start_row: usize, end_row: usize) -> Rect {
        let (mut y , mut height) = (0f64, 0f64);
        if start_row >= self.range.start_row {
            y = self.row_map.get(&start_row).map_or(0f64, |(y, _)| *y);
        }

        for row in start_row..end_row {
            let h = self.get_row_height(row);
            if h > 0f64 {
                height += h;
            }
        }

        return Rect {
            x: 0f64,
            y,
            width: self.width,
            height,
        }
    }

    pub fn rect_col(&self, start_col: usize, end_col: usize) -> Rect {
        let (mut x, mut width) = (0f64, 0f64);
        if start_col >= self.range.start_col {
            x = self.col_map.get(&start_col).map_or(0f64, |(x, _)| *x);
        }

        for col in start_col..end_col {
            let w = self.get_col_width(col);
            if w > 0f64 {
                width += w;
            }
        }

        return Rect {
            x,
            y: 0f64,
            width,
            height: self.height,
        }
    }

    pub fn rect(&self, r: &Range) -> Rect {
        let rect_row = self.rect_row(r.start_row, r.end_row);
        let rect_col = self.rect_col(r.start_col, r.end_col);
        return Rect {
            x: rect_col.x,
            y: rect_row.y,
            width: rect_col.width,
            height: rect_row.height,
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
        while cell.y < y {
            let h = self.get_row_height(cell.row);
            cell.row += 1;
            cell.y += h;
            cell.height = h;
        }

        cell.y -= cell.height;
        cell.row -= 1;

        // col
        while cell.x < x {
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