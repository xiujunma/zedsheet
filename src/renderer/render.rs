use wasm_bindgen::JsValue;
use BorderType::{Bottom, Horizontal, Left, Outside, Right, Top};
use crate::renderer::area::Area;
use crate::renderer::canvas::Canvas;
use crate::renderer::cell_render::cell_border_render;
use crate::renderer::range::Range;
use crate::renderer::table_renderer::{Border, BorderLine, BorderLineStyle, BorderType, Gridline, Rect, TableRenderer};
use crate::renderer::table_renderer::BorderType::{All, Inside, Vertical};

use super::border::border_ranges;
use super::table_renderer::Placement;
use super::viewport;

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
            let border_style = border.border_line;
            let border_color = border.color;
            border_ranges(area, &border, area_merges.clone()).into_iter().for_each(|(range, rect, border_type)| {
                render_border(canvas, area, &range, &rect, border_type, border_style.clone(), &border_color, None);
            });
        });
    }
}

pub fn render_area(placement: Placement, canvas: &Canvas, area: &Area, renderer: &TableRenderer) {

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