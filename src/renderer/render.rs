use wasm_bindgen::JsValue;

use BorderType::{Bottom, Horizontal, Left, Outside, Right, Top};

use crate::renderer::area::Area;
use crate::renderer::canvas::Canvas;
use crate::renderer::cell_render::{cell_border_render, cell_render, font_string};
use crate::renderer::range::{each_range, Range};
use crate::renderer::table_renderer::{Border, BorderLine, BorderLineStyle, BorderType, Cell, Gridline, Rect, SelectorRect, Style, TableRenderer};
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

pub fn render_selector(canvas: &Canvas, selector: &SelectorRect, viewport: &crate::renderer::viewport::Viewport, renderer: &TableRenderer) {
    // Find the area containing the selector cells
    for area in &viewport.areas {
        // Check if selector intersects with this area
        let min_r = selector.ri.min(selector.eri);
        let max_r = selector.ri.max(selector.eri);
        let min_c = selector.ci.min(selector.eci);
        let max_c = selector.ci.max(selector.eci);

        if max_r < area.range.start_row || min_r > area.range.end_row ||
           max_c < area.range.start_col || min_c > area.range.end_col {
            continue;
        }

        // Calculate the rect for the selection
        let sel_min_r = min_r.max(area.range.start_row);
        let sel_max_r = max_r.min(area.range.end_row);
        let sel_min_c = min_c.max(area.range.start_col);
        let sel_max_c = max_c.min(area.range.end_col);

        // Get positions (accounting for area offset)
        let mut sel_x = area.x;
        let mut sel_y = area.y;
        let mut sel_width = 0f64;
        let mut sel_height = 0f64;

        // Calculate Y position and height
        for row in sel_min_r..=sel_max_r {
            if row == sel_min_r {
                sel_y += area.row_map.get(&row).map_or(0f64, |(y, _)| *y);
            }
            sel_height += area.get_row_height(row);
        }

        // Calculate X position and width
        for col in sel_min_c..=sel_max_c {
            if col == sel_min_c {
                sel_x += area.col_map.get(&col).map_or(0f64, |(x, _)| *x);
            }
            sel_width += area.get_col_width(col);
        }

        // Draw selection border
        canvas.save();
        canvas.set_stroke_style("#0078d7");
        canvas.set_line_width(1.5);

        canvas.begin_path();
        canvas.rect(sel_x + 0.5, sel_y + 0.5, sel_width - 1.0, sel_height - 1.0);
        canvas.stroke();

        // Draw corner handle for resize (only on single cell selection)
        if selector.ri == selector.eri && selector.ci == selector.eci {
            let handle_size = 6f64;
            canvas.set_fill_style("#0078d7");
            canvas.fill_rect(
                sel_x + sel_width - handle_size,
                sel_y + sel_height - handle_size,
                handle_size,
                handle_size
            );
        }

        canvas.restore();
        break;
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

pub fn render_scrollbar(canvas: &Canvas, x: f64, y: f64, width: f64, height: f64, thumb_pos: f64, thumb_size: f64, vertical: bool) {
    canvas.save();

    // Draw scrollbar track
    canvas.set_fill_style("#f1f1f1");
    canvas.fill_rect(x, y, width, height);

    // Draw thumb
    canvas.set_fill_style("#c1c1c1");
    if vertical {
        canvas.fill_rect(x + 1f64, y + thumb_pos, width - 2f64, thumb_size);
    } else {
        canvas.fill_rect(x + thumb_pos, y + 1f64, thumb_size, height - 2f64);
    }

    canvas.restore();
}

pub fn render_cells(canvas: &Canvas, area: &Area, renderer: &TableRenderer) {
    let default_style = renderer.style.clone();

    area.each(|row, col, rect| {
        if let Some(cell) = renderer.data.get_cell(row, col) {
            let style = default_style.clone();
            // Render cell text
            canvas.save().begin_path().translate(rect.x, rect.y);
            canvas.rect(0f64, 0f64, rect.width, rect.height).clip(None);

            // Set fill for cell background
            if let Some(bgcolor) = style.bgcolor.clone() {
                canvas.set_fill_style(bgcolor.as_str());
                canvas.fill_rect(0f64, 0f64, rect.width, rect.height);
            }

            // Draw text
            let font = format!("{} {}px {}", style.font_family, style.font_size, if style.bold { "bold" } else { "" });
            canvas.set_font(&font)
                .set_fill_style(style.color.as_str())
                .set_text_align(&style.align.to_string())
                .set_text_baseline(&style.valign.to_string());

            let text = cell.value.clone();
            if !text.is_empty() {
                canvas.fill_text(&text, 5f64, rect.height / 2f64 + style.font_size as f64 / 3f64, None);
            }

            // Draw grid line
            render_cell_grid_line(canvas, &renderer.gridline, &rect);

            canvas.restore();
        } else {
            // Render empty cell
            render_cell_grid_line(canvas, &renderer.gridline, &rect);
        }
    });
}

pub fn render_area(placement: Placement, canvas: &Canvas, area: &Area, renderer: &TableRenderer) {
    let style = renderer.style.clone();
    match placement {
        Placement::RowHeader => {
            if renderer.row_header.width <= 0f64 {
                return;
            }
            // Render row headers (numbers)
            area.each_row(|row, y, height| {
                let rect = Rect { x: 0f64, y, width: renderer.row_header.width, height };

                // Render cell background
                if let Some(bgcolor) = renderer.header_style.bgcolor.clone() {
                    canvas.set_fill_style(bgcolor.as_str());
                    canvas.fill_rect(0f64, y, renderer.row_header.width, height);
                }

                // Draw text
                let font = format!("{} {}px {}", renderer.header_style.font_family, renderer.header_style.font_size, if renderer.header_style.bold { "bold" } else { "" });
                canvas.set_font(&font)
                    .set_fill_style(renderer.header_style.color.as_str())
                    .set_text_align(renderer.header_style.align.to_string().as_str())
                    .set_text_baseline(renderer.header_style.valign.to_string().as_str());
                canvas.fill_text(&format!("{}", row + 1), 5f64, y + height / 2f64 + renderer.header_style.font_size as f64 / 3f64, None);

                // Draw grid line
                render_cell_grid_line(canvas, &renderer.header_gridline, &rect);
            });
        }
        Placement::ColHeader => {
            if renderer.col_header.height <= 0f64 {
                return;
            }
            // Render column headers (letters A, B, C, ...)
            area.each_col(|col, x, width| {
                let rect = Rect { x, y: 0f64, width, height: renderer.col_header.height };
                let col_letter = crate::renderer::alphabets::string_at(col);

                // Render cell background
                if let Some(bgcolor) = renderer.header_style.bgcolor.clone() {
                    canvas.set_fill_style(bgcolor.as_str());
                    canvas.fill_rect(x, 0f64, width, renderer.col_header.height);
                }

                // Draw text
                let font = format!("{} {}px {}", renderer.header_style.font_family, renderer.header_style.font_size, if renderer.header_style.bold { "bold" } else { "" });
                canvas.set_font(&font)
                    .set_fill_style(renderer.header_style.color.as_str())
                    .set_text_align(renderer.header_style.align.to_string().as_str())
                    .set_text_baseline(renderer.header_style.valign.to_string().as_str());
                canvas.fill_text(&col_letter, x + 5f64, renderer.col_header.height / 2f64 + renderer.header_style.font_size as f64 / 3f64, None);

                // Draw grid line
                render_cell_grid_line(canvas, &renderer.header_gridline, &rect);
            });
        }
        Placement::Body => {
            // Render actual spreadsheet cells
            render_cells(canvas, area, renderer);
        }
        _ => {}
    }

    canvas.save()
        .translate(area.x, area.y)
        .set_fill_style(renderer.bgcolor.as_str())
        .rect(0.0, 0.0, area.width, area.height)
        .fill(None)
        .clip(None);

    let mut area_merges: Vec<Range> = vec![];
    let area_merge_render_params: Vec<(Cell, Rect, Style)> = vec![];
    if renderer.merges.len() > 0 {
        each_range(renderer.merges.clone(), |range| {
            if range.intersects(&area.range) {
                area_merges.push(range.clone());
                range.each(|r, c| {
                    if r > range.start_row || c > range.start_col {
                        area_merges.push(Range::new(r, c, r, c));
                    }
                });
            }
        });
    }

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

        // render-4 (body - scrollable area)
        render_area(Placement::Body, &canvas, &area4, renderer);

        // render-1 (frozen rows + column headers)
        render_area(Placement::Body, &canvas, &area1, renderer);
        render_area(Placement::ColHeader, &canvas, &header_area1, renderer);

        // render-3 (frozen columns + row headers)
        render_area(Placement::Body, &canvas, &area3, renderer);
        render_area(Placement::RowHeader, &canvas, &header_area3, renderer);

        // render 2 (frozen top-left corner)
        render_area(Placement::Body, &canvas, &area2, renderer);
        render_area(Placement::ColHeader, &canvas, &header_area21, renderer);
        render_area(Placement::RowHeader, &canvas, &header_area23, renderer);

        // render freeze lines
        let (row, col) = renderer.freeze;
        if row > 0 || col > 0 {
            render_lines(&canvas, &renderer.freeze_gridline, || {
                if col > 0 {
                    let freeze_x = area4.x;
                    canvas.line(freeze_x, 0.0, freeze_x, height);
                }

                if row > 0 {
                    let freeze_y = area4.y;
                    canvas.line(0.0, freeze_y, width, freeze_y);
                }
            });
        }

        // Render cell selector
        let selector = renderer.get_selector();
        if selector.ri > 0 || selector.ci > 0 || selector.eri > 0 || selector.eci > 0 {
            render_selector(&canvas, &selector, viewport, renderer);
        }

        // Render scrollbars (if content exceeds viewport)
        let total_row_height: f64 = (0..renderer.data.row_count).map(|i| renderer.row_height_at(i)).sum();
        let total_col_width: f64 = (0..renderer.data.col_count).map(|i| renderer.col_width_at(i)).sum();

        let scrollbar_size = 16f64;
        let has_vertical_scrollbar = total_row_height > height - renderer.col_header.height;
        let has_horizontal_scrollbar = total_col_width > width - renderer.row_header.width;

        if has_vertical_scrollbar {
            let bar_x = width - scrollbar_size;
            let bar_y = renderer.col_header.height;
            let bar_height = height - renderer.col_header.height - if has_horizontal_scrollbar { scrollbar_size } else { 0f64 };
            let content_height = total_row_height;
            let ratio = bar_height / content_height;
            let thumb_height = (bar_height * ratio).max(20f64).min(bar_height);
            let thumb_y = renderer.scroll_rows as f64 / renderer.data.row_count as f64 * (bar_height - thumb_height);

            render_scrollbar(&canvas, bar_x, bar_y, scrollbar_size, bar_height, thumb_y, thumb_height, true);
        }

        if has_horizontal_scrollbar {
            let bar_x = renderer.row_header.width;
            let bar_y = height - scrollbar_size;
            let bar_width = width - renderer.row_header.width - if has_vertical_scrollbar { scrollbar_size } else { 0f64 };
            let content_width = total_col_width;
            let ratio = bar_width / content_width;
            let thumb_width = (bar_width * ratio).max(20f64).min(bar_width);
            let thumb_x = renderer.scroll_cols as f64 / renderer.data.col_count as f64 * (bar_width - thumb_width);

            render_scrollbar(&canvas, bar_x, bar_y, bar_width, scrollbar_size, thumb_x, thumb_width, false);
        }

        // Render corner header background
        let (x, y) = (area2.x, area1.y);
        if x > 0.0 && y > 0.0 {
            let header_height = renderer.col_header.height;
            let header_width = renderer.row_header.width;

            if let Some(bgcolor) = renderer.header_style.bgcolor.clone() {
                canvas
                    .save()
                    .set_fill_style(bgcolor.as_str())
                    .rect(0.0, 0.0, header_width, header_height)
                    .fill(None)
                    .restore();

                render_lines(&canvas, &renderer.header_gridline, || {
                    canvas
                        .line(0.0, header_height, header_width, header_height)
                        .line(header_width, 0.0, header_width, header_height);
                });
            }
        }
    }
}