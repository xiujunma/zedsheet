#![allow(dead_code)]

use crate::renderer::area::Area;
use crate::renderer::range::Range;
use crate::renderer::table_renderer::{TableRenderer, ViewportCell};
use crate::renderer::table_renderer::Placement::{All, Body, ColHeader, RowHeader};

pub type GetRowHeight = fn(usize) -> f64;
pub type GetColWidth = fn(usize) -> f64;

pub struct Viewport {
    pub areas: Vec<Area>,
    pub header_areas: Vec<Area>,
    pub render: &'static mut TableRenderer<'static>,
}

impl Viewport {
    pub fn new(render: &'static mut TableRenderer) -> Viewport {
        let (tx, ty) = (render.row_header.width, render.col_header.height);
        let (frow, fcol) = render.freeze;
        let start_row = render.start_row;
        let start_col = render.start_col;
        let rows = render.rows;
        let cols = render.cols;
        let width = render.width;
        let height = render.height;

        let get_row_height: GetRowHeight = |row: usize| -> f64 {
            render.row_height_at(row)
        };

        let get_col_width: GetColWidth = |col: usize| -> f64 {
            render.col_width_at(col)
        };

        let area2 = Area::new(Range {
            start_row,
            start_col,
            end_row: frow - 1,
            end_col: fcol - 1,
        }, tx, ty, 0f64, 0f64, get_row_height, get_col_width);

        let (start_row_4, start_col_4) = (frow + render.scroll_rows, fcol + render.scroll_cols);

        // end row
        let mut y = area2.height + ty;
        let end_row = start_row_4;
        while y < height && end_row < rows {
            y += get_row_height(end_row);
            end_row += 1;
        }

        // end col
        let mut x = area2.width + tx;
        let end_col = start_col_4;
        while x < width && end_col < cols {
            x += get_col_width(end_col);
            end_col += 1;
        }

        // area4
        let x4 = tx + area2.width;
        let y4 = ty + area2.height;
        let mut w4 = width - x4;
        let mut h4 = height - y4;
        if end_col == cols {
            w4 -= width - x;
        }

        if end_row == rows {
            h4 -= height - y;
        }

        end_col -= 1;
        end_row -= 1;

        let area4 = Area::new(Range {
            start_row: start_row_4,
            start_col: start_col_4,
            end_row,
            end_col,
        }, x4, y4, w4, h4, get_row_height, get_col_width);

        let area1 = Area::new(Range {
            start_row,
            start_col: start_col_4,
            end_row: frow - 1,
            end_col,
        }, x4, ty, w4, 0f64, get_row_height, get_col_width);


        let area3 = Area::new(Range {
            start_row: start_row_4,
            start_col,
            end_row,
            end_col: fcol - 1,
        }, tx, y4, 0f64, h4, get_row_height, get_col_width);


        // header areas
        let row_header = &render.row_header;
        let col_header = &render.col_header;

        let get_col_header_row: GetRowHeight = |row: usize| -> f64 {
            col_header.height / col_header.rows
        };

        let get_row_header_col: GetColWidth = |col: usize| -> f64 {
            row_header.width / row_header.cols
        };

        // 1, 2-1, 2-3, 3
        let header_area1 = Area::new(Range {
            start_row: 0,
            start_col: area1.range.start_col,
            end_row: col_header.rows - 1,
            end_col: area1.range.end_col,
        }, area4.x, 0f64, area4.width, 0f64, get_col_header_row, get_col_width);

        let header_area2 = Area::new(Range {
            start_row: 0,
            start_col: area2.range.start_col,
            end_row: col_header.rows - 1,
            end_col: area2.range.end_col,
        }, area2.x, 0f64, area2.width, 0f64, get_col_header_row, get_col_width);

        let header_area3 = Area::new(Range {
            start_row: 0,
            start_col: area3.range.start_col,
            end_row: col_header.rows - 1,
            end_col: area3.range.end_col,
        }, area3.x, 0f64, area3.width, 0f64, get_col_header_row, get_col_width);

        let header_area4 = Area::new(Range {
            start_row: 0,
            start_col: area4.range.start_col,
            end_row: col_header.rows - 1,
            end_col: area4.range.end_col,
        }, area4.x, 0f64, area4.width, 0f64, get_col_header_row, get_col_width);


        Viewport {
            areas: vec![area1, area2, area3, area4],
            header_areas: vec![header_area1, header_area2, header_area3, header_area4],
            render
        }
    }

    fn in_areas(&self, row: usize, col: usize) -> bool {
        for it in self.areas  {
            if it.range.contains(row, col) {
                return true;
            }
        }
        return false;
    }

    fn cell_at(&self, x: f64, y: f64) -> Option<ViewportCell> {
        let a2 = self.areas.get(1).unwrap();
        let [ha1, ha21, ha23, ha3] = self.header_areas.as_slice();

        if x < a2.x && y < a2.y {
            return Option::from(ViewportCell {
                placement: All,
                row: 0,
                col: 0,
                x: 0f64,
                y: 0f64,
                width: a2.x,
                height: a2.y
            })
        }

        if x < a2.x {
            let header_area = if ha23.contains_y(y) {
                ha23
            } else {
                ha3
            };

            let area_cell = header_area.cell_at(x, y);

            return Some(ViewportCell {
                placement: RowHeader,
                row: area_cell.row,
                col: area_cell.col,
                x: area_cell.x,
                y: area_cell.y,
                width: area_cell.width,
                height: area_cell.height,
            });
        }

        if y < a2.y {
            let header_area = if ha21.contains_x(x) {
                ha21
            } else {
                ha1
            };

            let area_cell = header_area.cell_at(x, y);

            return Some(ViewportCell {
                placement: ColHeader,
                row: area_cell.row,
                col: area_cell.col,
                x: area_cell.x,
                y: area_cell.y,
                width: area_cell.width,
                height: area_cell.height,
            });
        }

        for area in self.areas {
            if area.contains(x, y) {
                let area_cell = area.cell_at(x, y);
                return Some(ViewportCell {
                    placement: Body,
                    row: area_cell.row,
                    col: area_cell.col,
                    x: area_cell.x,
                    y: area_cell.y,
                    width: area_cell.width,
                    height: area_cell.height,
                });
            }
        }

        None
    }
}