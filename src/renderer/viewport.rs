#![allow(dead_code)]

use crate::core::data_proxy::DataProxy;
use crate::renderer::area::Area;
use crate::renderer::range::Range;
use crate::renderer::table_renderer::Placement;
use crate::renderer::table_renderer::ViewportCell;

use super::table_renderer::{ColHeader, RowHeader};

pub struct Viewport {
    pub areas: Vec<Area>,
    pub header_areas: Vec<Area>,
}

impl Viewport {
    pub fn new(
        data: DataProxy,
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
        // tx/ty: width/height reserved for the row/column headers.
        let (tx, ty) = (row_header.width, col_header.height);
        // (frow, fcol) are COUNTS of frozen rows/cols. 0 means "no freeze".
        let (frow, fcol) = freeze;

        // Ranges below use EXCLUSIVE end bounds (start..end), matching Range::each_*.
        // area2: frozen top-left corner = rows [start_row, frow) x cols [start_col, fcol).
        let area2: Area = Area::new(
            data.clone(),
            Range {
                start_row,
                start_col,
                end_row: start_row.max(frow),
                end_col: start_col.max(fcol),
            },
            tx,
            ty,
            0f64,
            0f64,
        );

        // Body scroll origin sits just past the frozen panes.
        let (start_row_4, start_col_4) = (frow + scroll_rows, fcol + scroll_cols);

        // Walk rows/cols until the body viewport is filled; end_* is the first
        // row/col that no longer fits (exclusive upper bound).
        let mut y = area2.height + ty;
        let mut end_row = start_row_4;
        while y < height && end_row < rows {
            y += data.get_row_height(end_row);
            end_row += 1;
        }

        let mut x = area2.width + tx;
        let mut end_col = start_col_4;
        while x < width && end_col < cols {
            x += data.get_col_width(end_col);
            end_col += 1;
        }

        // area4: the scrollable body.
        let x4 = tx + area2.width;
        let y4 = ty + area2.height;
        let w4 = width - x4;
        let h4 = height - y4;

        let area4 = Area::new(
            data.clone(),
            Range {
                start_row: start_row_4,
                start_col: start_col_4,
                end_row,
                end_col,
            },
            x4,
            y4,
            w4,
            h4,
        );

        // area1: frozen rows above the body (frozen rows x body cols).
        let area1 = Area::new(
            data.clone(),
            Range {
                start_row,
                start_col: start_col_4,
                end_row: start_row.max(frow),
                end_col,
            },
            x4,
            ty,
            w4,
            0f64,
        );

        // area3: frozen cols left of the body (body rows x frozen cols).
        let area3 = Area::new(
            data.clone(),
            Range {
                start_row: start_row_4,
                start_col,
                end_row,
                end_col: start_col.max(fcol),
            },
            tx,
            y4,
            0f64,
            h4,
        );

        // header areas (column headers), one per body/frozen column span.
        let header_rows = col_header.rows;
        let header_area1 = Area::new(
            data.clone(),
            Range {
                start_row: 0,
                start_col: area1.range.start_col,
                end_row: header_rows,
                end_col: area1.range.end_col,
            },
            area4.x,
            0f64,
            area4.width,
            0f64,
        );

        let header_area2 = Area::new(
            data.clone(),
            Range {
                start_row: 0,
                start_col: area2.range.start_col,
                end_row: header_rows,
                end_col: area2.range.end_col,
            },
            area2.x,
            0f64,
            area2.width,
            0f64,
        );

        let header_area3 = Area::new(
            data.clone(),
            Range {
                start_row: 0,
                start_col: area3.range.start_col,
                end_row: header_rows,
                end_col: area3.range.end_col,
            },
            area3.x,
            0f64,
            area3.width,
            0f64,
        );

        let header_area4 = Area::new(
            data.clone(),
            Range {
                start_row: 0,
                start_col: area4.range.start_col,
                end_row: header_rows,
                end_col: area4.range.end_col,
            },
            area4.x,
            0f64,
            area4.width,
            0f64,
        );

        Viewport {
            areas: vec![area1, area2, area3, area4],
            header_areas: vec![header_area1, header_area2, header_area3, header_area4],
        }
    }

    fn in_areas(&self, row: usize, col: usize) -> bool {
        for it in &self.areas {
            if it.range.contains(row, col) {
                return true;
            }
        }
        false
    }

    fn cell_at(&self, x: f64, y: f64) -> Option<ViewportCell> {
        let a2 = self.areas.get(1).unwrap();
        // let [ha1, ha21, ha23, ha3] = self.header_areas.as_slice();

        let ha1 = self.header_areas.first().unwrap();
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
                height: a2.y,
            });
        }

        if x < a2.x {
            let header_area = if ha23.contains_y(y) { ha23 } else { ha3 };

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
            let header_area = if ha21.contains_x(x) { ha21 } else { ha1 };

            let area_cell = header_area.cell_at(x, y);

            return Some(ViewportCell {
                placement: Placement::ColHeader,
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
