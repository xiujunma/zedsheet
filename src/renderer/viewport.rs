#![allow(dead_code)]

use crate::data::table_data::TableData;
use crate::renderer::area::Area;
use crate::renderer::range::Range;
use crate::renderer::table_renderer::ViewportCell;
use crate::renderer::table_renderer::Placement;

use super::table_renderer::{ RowHeader, ColHeader};


pub struct Viewport {
    pub areas: Vec<Area>,
    pub header_areas: Vec<Area>,
}

impl Viewport {
    pub fn new(
        data: TableData,
        freeze: (usize, usize),
        start_row: usize,
        start_col: usize,
        rows: usize,
        cols: usize,
        width: f64,
        height: f64,
        scroll_rows: usize,
        scroll_cols: usize,
        row_header: RowHeader,
        col_header: ColHeader,
    ) -> Viewport {
        let (tx, ty) = (row_header.width, col_header.height);
        let (frow, fcol) = freeze;
        let area2: Area = Area::new(data.clone(), Range {
            start_row,
            start_col,
            end_row: frow - 1,
            end_col: fcol - 1,
        }, tx, ty, 0f64, 0f64);

        let (start_row_4, start_col_4) = (frow + scroll_rows, fcol + scroll_cols);

        // end row
        let mut y = area2.height + ty;
        let mut end_row = start_row_4;
        while y < height && end_row < rows {
            y += data.get_row(end_row).unwrap().height;
            end_row += 1;
        }

        // end col
        let mut x = area2.width + tx;
        let mut end_col = start_col_4;
        while x < width && end_col < cols {
            x += data.get_col(end_col).unwrap().width;
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

        let area4 = Area::new(data.clone(), 
            Range {
            start_row: start_row_4,
            start_col: start_col_4,
            end_row,
            end_col,
        }, x4, y4, w4, h4);

        let area1 = Area::new(data.clone(), 
            Range {
            start_row,
            start_col: start_col_4,
            end_row: frow - 1,
            end_col,
        }, x4, ty, w4, 0f64);


        let area3 = Area::new(data.clone(), 
            Range {
            start_row: start_row_4,
            start_col,
            end_row,
            end_col: fcol - 1,
        }, tx, y4, 0f64, h4);


        // header areas

        // 1, 2-1, 2-3, 3
        let header_area1 = Area::new(data.clone(),
            Range {
            start_row: 0,
            start_col: area1.range.start_col,
            end_row: col_header.rows - 1,
            end_col: area1.range.end_col,
        }, area4.x, 0f64, area4.width, 0f64);

        let header_area2 = Area::new(data.clone(), Range {
            start_row: 0,
            start_col: area2.range.start_col,
            end_row: col_header.rows - 1,
            end_col: area2.range.end_col,
        }, area2.x, 0f64, area2.width, 0f64);

        let header_area3 = Area::new(data.clone(), Range {
            start_row: 0,
            start_col: area3.range.start_col,
            end_row: col_header.rows - 1,
            end_col: area3.range.end_col,
        }, area3.x, 0f64, area3.width, 0f64);

        let header_area4 = Area::new(data.clone(), Range {
            start_row: 0,
            start_col: area4.range.start_col,
            end_row: col_header.rows - 1,
            end_col: area4.range.end_col,
        }, area4.x, 0f64, area4.width, 0f64);


        Viewport {
            areas: vec![area1, area2, area3, area4],
            header_areas: vec![header_area1, header_area2, header_area3, header_area4],
            // render
        }
    }

    fn in_areas(&self, row: usize, col: usize) -> bool {
        for it in &self.areas  {
            if it.range.contains(row, col) {
                return true;
            }
        }
        return false;
    }

    fn cell_at(&self, x: f64, y: f64) -> Option<ViewportCell> {
        let a2 = self.areas.get(1).unwrap();
        // let [ha1, ha21, ha23, ha3] = self.header_areas.as_slice();

        let ha1 = self.header_areas.get(0).unwrap();
        let ha21 = self.header_areas.get(1).unwrap();
        let ha23 = self.header_areas.get(2).unwrap();
        let ha3 = self.header_areas.get(3).unwrap();

        if x < a2.x && y < a2.y {
            return Option::from(ViewportCell {
                placement: Placement::All,
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
                placement: Placement::RowHeader,
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
                placement: Placement:: ColHeader,
                row: area_cell.row,
                col: area_cell.col,
                x: area_cell.x,
                y: area_cell.y,
                width: area_cell.width,
                height: area_cell.height,
            });
        }

        for area in &self.areas {
            if area.contains(x, y) {
                let area_cell = area.cell_at(x, y);
                return Some(ViewportCell {
                    placement: Placement::Body,
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