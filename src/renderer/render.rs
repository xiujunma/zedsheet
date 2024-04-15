use wasm_bindgen::JsValue;

use BorderType::{Bottom, Horizontal, Left, Outside, Right, Top};

use crate::renderer::area::Area;
use crate::renderer::canvas::Canvas;
use crate::renderer::cell_render::cell_border_render;
use crate::renderer::range::{each_range, Range};
use crate::renderer::table_renderer::{Border, BorderLine, BorderLineStyle, BorderType, Cell, Gridline, Rect, Style, TableRenderer};
use crate::renderer::table_renderer::BorderType::{All, Inside, Vertical};

use super::border::border_ranges;
use super::table_renderer::Placement;

pub trait AreaRenderer {
    fn cell(&self, row_index: usize, col_index: usize) -> Option<Cell>;
    fn get_merges(&self) -> Vec<String>;
    fn cell_render(&self, canvas: &Canvas, rect: &Rect, cell: &Cell, text: &str) -> bool;
}

pub fn render_lines(canvas: &Canvas, gridline: &Gridline, cb: impl Fn()) {
    if gridline.width > 0f64 {
        canvas
            .save()
            .begin_path();

        canvas.ctx.set_line_width(gridline.width - 0.5);
        canvas.ctx.set_stroke_style(&JsValue::from_str(&gridline.color));

        cb();
        canvas.restore();
    }
}

pub fn render_cell_grid_line(canvas: &Canvas, gridline: &Gridline, rect: &Rect) {
    render_lines(canvas, gridline, || {
        canvas
            .translate(rect.x, rect.y)
            .line(rect.width, 0f64, rect.width, rect.height)
            .line(0f64, rect.height, rect.width, rect.height);
    });
}

pub fn render_border(canvas: &Canvas, area: &Area, range: &Range, border_rect: &Rect, border_type: BorderType, line_style: BorderLineStyle, color: &str, auto_align: Option<bool>) {
    if border_type == Outside || border_type == All {
        let border_line = BorderLine {
            left: Some((line_style.clone(), color.to_string())),
            top: Some((line_style.clone(), color.to_string())),
            right: Some((line_style.clone(), color.to_string())),
            bottom: Some((line_style.clone(), color.to_string())),
        };
        cell_border_render(canvas, border_rect, &border_line, auto_align);
    } else if border_type == Left {
        let border_line = BorderLine {
            left: Some((line_style.clone(), color.to_string())),
            top: None,
            right: None,
            bottom: None,
        };
        cell_border_render(canvas, border_rect, &border_line, auto_align);
    } else if border_type == Top {
        let border_line = BorderLine {
            left: None,
            top: Some((line_style.clone(), color.to_string())),
            right: None,
            bottom: None,
        };
        cell_border_render(canvas, border_rect, &border_line, auto_align);
    } else if border_type == Right {
        let border_line = BorderLine {
            left: None,
            top: None,
            right: Some((line_style.clone(), color.to_string())),
            bottom: None,
        };
        cell_border_render(canvas, border_rect, &border_line, auto_align);
    } else if border_type == Bottom {
        let border_line = BorderLine {
            left: None,
            top: None,
            right: None,
            bottom: Some((line_style.clone(), color.to_string())),
        };
        cell_border_render(canvas, border_rect, &border_line, auto_align);
    }

    if border_type == All || border_type == Inside || border_type == Horizontal || border_type == Vertical {
        if border_type != Horizontal {
            range.each_col(move |index| {
                if index < range.end_col {
                    let mut r1 = range.clone();
                    r1.end_col = index;
                    r1.start_col = index;
                    if r1.intersects(&area.range) {
                        let rect = area.rect(&r1);
                        cell_border_render(canvas, &rect, &BorderLine {
                            left: None,
                            top: None,
                            right: Some((line_style, color.to_string())),
                            bottom: None,
                        }, auto_align)
                    }
                }
            }, None);
        }

        if border_type != Vertical {
            range.each_row(|index| {
                if index < range.end_row {
                    let mut r1 = range.clone();
                    r1.end_row = index;
                    r1.start_row = index;
                    if r1.intersects(&area.range) {
                        let rect = area.rect(&r1);
                        cell_border_render(canvas, &rect, &BorderLine {
                            left: None,
                            top: None,
                            right: None,
                            bottom: Some((line_style.clone(), color.to_string())),
                        }, auto_align)
                    }
                }
            }, None);
        }
    }
}

pub fn render_borders(canvas: &Canvas, area: &Area, borders: Option<Vec<Border>>, area_merges: Vec<Range>) {
    if let Some(borders) = borders {
        borders.into_iter().for_each(|border| {
            let border_style = border.clone().border_line;
            let border_color = border.clone().color;
            border_ranges(area, &border, area_merges.clone()).into_iter().for_each(|(range, rect, border_type)| {
                render_border(canvas, area, &range, &rect, border_type, border_style.clone(), &border_color, None);
            });
        });
    }
}

pub fn render_area(placement: Placement, canvas: &Canvas, area: &Area, renderer: &TableRenderer) {
    // TODO
    let style = renderer.style.clone();
    match placement {
        Placement::RowHeader => {
            if renderer.row_header.width <= 0f64 {
                return;
            } else {
                
            }
        }

        Placement::ColHeader => {
            if renderer.col_header.height <= 0f64 {
                return;
            }
        }

        _default => {
        }
    }

    canvas.save()
        .translate(area.x, area.y)
        .set_fill_style(renderer.bgcolor.as_str())
        .rect(0.0, 0.0, area.width, area.height)
        .fill(None)
        .clip(None);

    let merge_cell_style = |r: usize, c: usize, cell: &Cell|-> Style {
        //TODO
        style.clone()
    };

    let mut area_merges: Vec<Range> = vec![];
    let mut area_merge_render_params: Vec<(Cell, Rect, Style)> = vec![];
    // let cell_merges: Vec<Range> = vec![];
    if renderer.merges.len() > 0 {
        each_range(renderer.merges.clone(), |range| {
            if range.intersects(&area.range) {
                let cell_v = renderer.data.get_cell(range.start_row, range.start_col).unwrap();
                let cell_style = merge_cell_style(range.start_row, range.start_col, &cell_v);
                let cell_rect = area.rect(&range);
                area_merge_render_params.push((cell_v, cell_rect, cell_style));
                area_merges.push(range.clone());

                range.each(|r, c| {
                    if r > range.start_row || c > range.start_col {
                        area_merges.push(Range::new(r, c, r, c));
                    }
                });
            }
        });
    }

    let render = |cell: &Cell, rect: &Rect, style: &Style| {
        if placement == Placement::Body {
            render_cell_grid_line(canvas, &renderer.gridline, rect);
            // TODO
        } else {

        }
    };

    render_borders(canvas, area, Some(renderer.borders.clone()), area_merges.clone());
    canvas.restore();
}

pub fn render(renderer: &TableRenderer) {
    let width = renderer.width;
    let height = renderer.height;
    let target = renderer.target.clone();
    let scale = renderer.scale;
    if let Some(viewport) = &renderer.viewport {
        let canvas = Canvas::new(target, scale);
        canvas.set_size(width, height);

        let area1 = viewport.areas.get(0).unwrap();
        let area2 = viewport.areas.get(1).unwrap();
        let area3 = viewport.areas.get(2).unwrap();
        let area4 = viewport.areas.get(3).unwrap();

        let header_area1 = viewport.header_areas.get(0).unwrap();
        let header_area21 = viewport.header_areas.get(1).unwrap();
        let header_area23 = viewport.header_areas.get(2).unwrap();
        let header_area3 = viewport.header_areas.get(3).unwrap();

        // render-4
        render_area(Placement::Body, &canvas, &area4, renderer);

        // render-1
        render_area(Placement::Body, &canvas, &area1, renderer);
        render_area(Placement::ColHeader, &canvas, &header_area1, renderer);

        // render-3
        render_area(Placement::Body, &canvas, &area3, renderer);
        render_area(Placement::RowHeader, &canvas, &header_area3, renderer);

        // render 2
        render_area(Placement::Body, &canvas, &area2, renderer);
        render_area(Placement::ColHeader, &canvas, &header_area21, renderer);
        render_area(Placement::RowHeader, &canvas, &header_area23, renderer);

        // render freeze
        let (row, col) = renderer.freeze;
        if row > 0 || col > 0 {
            render_lines(&canvas, &renderer.freeze_gridline, || {
                if col > 0 {
                    canvas.line(0.0, area4.y, width, area4.y);
                }

                if row > 0 {
                    canvas.line(area4.x, 0.0, area4.x, height);
                }
            });
        }

        let (x, y) = (area2.x, area1.y);
        if x > 0.0 && y > 0.0 {
            let height = renderer.col_header.height;
            let width = renderer.row_header.width;

            if let Some(bgcolor) = renderer.header_style.bgcolor.clone() {
                canvas
                    .save()
                    .set_fill_style(bgcolor.as_str())
                    .rect(0.0, 0.0, width, height)
                    .fill(None)
                    .restore();

                render_lines(&canvas, &renderer.header_gridline, || {
                    canvas
                        .line(0.0, height, width, height)
                        .line(width, 0.0, width, height);
                });
            }
        }
    }
}