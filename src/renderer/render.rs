use BorderType::{Bottom, Horizontal, Left, Outside, Right, Top};

use crate::renderer::area::Area;
use crate::renderer::canvas::Canvas;
use crate::renderer::cell_render::cell_border_render;
use crate::renderer::range::{each_range, Range};
use crate::renderer::table_renderer::{Border, BorderLine, BorderLineStyle, BorderType, Cell, Gridline, Rect, SelectorRect, Style, TableRenderer};
use crate::renderer::table_renderer::BorderType::{All, Inside, Vertical};

use super::border::border_ranges;
use super::table_renderer::Placement;
use super::alphabets::string_at;

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
        canvas.ctx.set_stroke_style_str(&gridline.color);

        cb();
        canvas.restore();
    }
}

pub fn render_selector(canvas: &Canvas, _selector: &SelectorRect, viewport: &crate::renderer::viewport::Viewport, renderer: &TableRenderer) {
    // Issue #19: draw every range in `multi_range`, with the fill handle on the
    // last one. Falls back to the single-rect `selector` when there are no
    // Ctrl/Cmd-added ranges.
    renderer.set_fill_handle_rect(None);

    let ranges = renderer.selection_ranges();
    if ranges.is_empty() {
        return;
    }
    let last_idx = ranges.len() - 1;

    for (i, (r0, c0, r1, c1)) in ranges.iter().enumerate() {
        let is_last = i == last_idx;
        for area in &viewport.areas {
            // Skip empty areas (exclusive end bound => start >= end means no cells).
            if area.range.start_row >= area.range.end_row || area.range.start_col >= area.range.end_col {
                continue;
            }

            // The area's last visible row/col (end bound is exclusive).
            let area_last_row = area.range.end_row - 1;
            let area_last_col = area.range.end_col - 1;

            let min_r = *r0;
            let max_r = *r1;
            let min_c = *c0;
            let max_c = *c1;

            if max_r < area.range.start_row || min_r > area_last_row ||
               max_c < area.range.start_col || min_c > area_last_col {
                continue;
            }

            // Clamp the selection to the cells actually present in this area.
            let sel_min_r = min_r.max(area.range.start_row);
            let sel_max_r = max_r.min(area_last_row);
            let sel_min_c = min_c.max(area.range.start_col);
            let sel_max_c = max_c.min(area_last_col);

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
                sel_height += area.row_map.get(&row).map_or(0f64, |(_, h)| *h);
            }

            // Calculate X position and width
            for col in sel_min_c..=sel_max_c {
                if col == sel_min_c {
                    sel_x += area.col_map.get(&col).map_or(0f64, |(x, _)| *x);
                }
                sel_width += area.col_map.get(&col).map_or(0f64, |(_, w)| *w);
            }

            // Draw selection border
            canvas.save();
            canvas.set_stroke_style("#0078d7");
            canvas.set_line_width(1.5);

            canvas.begin_path();
            canvas.rect(sel_x + 0.5, sel_y + 0.5, sel_width - 1.0, sel_height - 1.0);
            canvas.stroke();

            // Fill handle: a small square at the selection's bottom-right corner.
            // Only the last range gets the handle; otherwise the first range's
            // handle would persist and confuse the user.
            if is_last {
                let handle_size = 6f64;
                let hx = sel_x + sel_width - handle_size;
                let hy = sel_y + sel_height - handle_size;
                canvas.set_fill_style("#0078d7");
                canvas.fill_rect(hx, hy, handle_size, handle_size);
                renderer.set_fill_handle_rect(Some((hx, hy, handle_size, handle_size)));
            }

            canvas.restore();
            break;
        }
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

/// Render body cells with per-cell styles, formula values and merges. Assumes
/// the caller has translated the canvas to the area origin; `rect` coordinates
/// are area-relative.
pub fn render_cells(canvas: &Canvas, area: &Area, renderer: &TableRenderer) {
    let grid = &renderer.gridline;
    let pad = 5f64;

    area.each(|row, col, rect| {
        // Merge handling: skip cells covered by (but not the origin of) a merge.
        let merge = renderer.data.cell_merge(row, col);
        let mut draw_rect = rect;
        if let Some(m) = &merge {
            if row != m.sri || col != m.sci {
                return; // covered cell: nothing to draw (origin paints over it)
            }
            // Expand the draw rect to span the full merged range.
            let mut w = 0f64;
            for c in m.sci..=m.eci {
                w += renderer.col_width_at(c);
            }
            let mut h = 0f64;
            for r in m.sri..=m.eri {
                h += renderer.row_height_at(r);
            }
            draw_rect = Rect { x: rect.x, y: rect.y, width: w, height: h };
        }

        let style = renderer.data.get_cell_style(row, col);

        // Background fill (only when non-white, to preserve gridlines elsewhere).
        if let Some(bg) = &style.bgcolor {
            if !is_white(bg) {
                canvas.set_fill_style(bg.as_str());
                canvas.fill_rect(draw_rect.x, draw_rect.y, draw_rect.width, draw_rect.height);
            }
        }

        // Cell text (formulas resolved to their value).
        let text = renderer.data.cell_display_value(row, col);

        // Resolve the effective font size: shrink-to-fit (issue #25) shrinks
        // the font iteratively until the text fits the cell width without
        // wrapping. Only applies when text_wrap is off and the cell has
        // text; empty cells keep the declared size.
        let mut font_size = style.font_size as f64;
        if style.shrink_to_fit && !style.text_wrap && !text.is_empty() {
            let max_w = (draw_rect.width - 2f64 * pad - style.indent as f64).max(1f64);
            let max_h = (draw_rect.height - 2f64 * pad).max(1f64);
            // Shrink the font until the text fits the cell (width + one line's
            // height). The canvas font MUST be re-set each iteration, otherwise
            // measure_text_width keeps reporting the original width and the loop
            // walks straight to the 2px floor (the bug this fixes).
            loop {
                let trial_font = format!(
                    "{}{}{}px {}",
                    if style.italic { "italic " } else { "" },
                    if style.bold { "bold " } else { "" },
                    font_size,
                    style.font_family
                );
                canvas.set_font(&trial_font);
                let w = canvas.measure_text_width(&text);
                let line_h = font_size * 1.3;
                if (w <= max_w && line_h <= max_h) || font_size <= 2.0 {
                    break;
                }
                font_size = (font_size - 1.0).max(2.0);
            }
        }

        if !text.is_empty() {
            let font = format!(
                "{}{}{}px {}",
                if style.italic { "italic " } else { "" },
                if style.bold { "bold " } else { "" },
                font_size,
                style.font_family
            );
            // Indent adds to the left padding (issue #25). Right/center
            // alignment ignores indent (Excel behavior).
            // Reserve right-edge space for the list-validity ▼ glyph
            // (issue #9) so cell text doesn't slide under it.
            let has_list_glyph = renderer
                .data
                .get_note(row, col)
                .is_none()
                && renderer
                    .data
                    .validations
                    .get(row, col)
                    .map(|v| v.validator.type_ == "list")
                    .unwrap_or(false);
            let right_inset = if has_list_glyph { 14f64 } else { 0f64 };

            let left_pad = pad + style.indent as f64;
            let (tx, talign) = match style.align.as_str() {
                "center" => (draw_rect.x + (draw_rect.width - right_inset) / 2f64, "center"),
                "right" => (draw_rect.x + draw_rect.width - pad - right_inset, "right"),
                _ => (draw_rect.x + left_pad, "left"),
            };

            // Hyperlink cells render in link blue and underlined.
            let is_link = renderer.data.get_link(row, col).is_some();
            let text_color: &str = if is_link { "#1a73e8" } else { style.color.as_str() };
            let underline_on = style.underline || is_link;

            canvas.save();

            // Rotation (issue #25): pivot around the cell center. Apply it
            // BEFORE the clip so the clip rectangle rotates with the text —
            // otherwise rotated text is clipped to the unrotated cell box and
            // gets cut off (e.g. vertical text in a short cell).
            if let Some(angle) = style.rotation {
                if angle.abs() > 1e-9 {
                    let cx = draw_rect.x + draw_rect.width / 2f64;
                    let cy = draw_rect.y + draw_rect.height / 2f64;
                    canvas.translate(cx, cy);
                    canvas.rotate(angle * std::f64::consts::PI / 180f64);
                    canvas.translate(-cx, -cy);
                }
            }

            canvas.begin_path();
            canvas.rect(
                draw_rect.x,
                draw_rect.y,
                draw_rect.width - right_inset,
                draw_rect.height,
            );
            canvas.clip(None);

            canvas
                .set_font(&font)
                .set_fill_style(text_color)
                .set_text_align(talign);

            if style.text_wrap {
                // Wrap the text into lines that fit the cell width.
                canvas.set_text_baseline("top");
                let max_w = (draw_rect.width - 2f64 * pad - style.indent as f64).max(1f64);
                let lines = wrap_text(canvas, &text, max_w);
                let line_h = font_size * 1.3;
                let total_h = line_h * lines.len() as f64;
                let start_y = match style.valign.as_str() {
                    "bottom" => draw_rect.y + draw_rect.height - pad - total_h,
                    "middle" => draw_rect.y + (draw_rect.height - total_h) / 2f64,
                    _ => draw_rect.y + pad,
                }
                .max(draw_rect.y + pad);
                for (i, line) in lines.iter().enumerate() {
                    canvas.fill_text(line, tx, start_y + i as f64 * line_h, None);
                }
            } else {
                let (ty, tbaseline) = match style.valign.as_str() {
                    "top" => (draw_rect.y + pad, "top"),
                    "bottom" => (draw_rect.y + draw_rect.height - pad, "bottom"),
                    _ => (draw_rect.y + draw_rect.height / 2f64, "middle"),
                };
                canvas.set_text_baseline(tbaseline);
                canvas.fill_text(&text, tx, ty, None);

                // Underline / strike-through across the text width.
                if underline_on || style.strike {
                    let tw = canvas.measure_text_width(&text);
                    let (lx0, lx1) = match talign {
                        "center" => (tx - tw / 2f64, tx + tw / 2f64),
                        "right" => (tx - tw, tx),
                        _ => (tx, tx + tw),
                    };
                    canvas.set_stroke_style(text_color);
                    canvas.set_line_width(1.0);
                    if underline_on {
                        let ly = ty + font_size * 0.4;
                        canvas.line(lx0, ly, lx1, ly);
                    }
                    if style.strike {
                        canvas.line(lx0, ty, lx1, ty);
                    }
                }
            }
            canvas.restore();
        }

        // Right + bottom gridlines (spanning the merged rect where applicable).
        canvas.set_stroke_style(grid.color.as_str());
        canvas.set_line_width(grid.width);
        canvas.line(draw_rect.x + draw_rect.width, draw_rect.y, draw_rect.x + draw_rect.width, draw_rect.y + draw_rect.height);
        canvas.line(draw_rect.x, draw_rect.y + draw_rect.height, draw_rect.x + draw_rect.width, draw_rect.y + draw_rect.height);

        // Per-cell borders (drawn on top of the gridlines).
        if let Some(b) = &style.border {
            let x0 = draw_rect.x;
            let y0 = draw_rect.y;
            let x1 = draw_rect.x + draw_rect.width;
            let y1 = draw_rect.y + draw_rect.height;
            draw_border_side(canvas, &b.top, x0, y0, x1, y0);
            draw_border_side(canvas, &b.bottom, x0, y1, x1, y1);
            draw_border_side(canvas, &b.left, x0, y0, x0, y1);
            draw_border_side(canvas, &b.right, x1, y0, x1, y1);
        }

        // Note marker: a small red triangle in the cell's top-right corner.
        if renderer.data.get_note(row, col).is_some() {
            let x1 = draw_rect.x + draw_rect.width;
            let y0 = draw_rect.y;
            canvas
                .set_fill_style("#e53935")
                .begin_path()
                .move_to(x1 - 6f64, y0)
                .line_to(x1, y0)
                .line_to(x1, y0 + 6f64)
                .close_path()
                .fill(None);
        }

        // List-validity dropdown glyph (issue #9): a small ▼ in the cell's
        // right edge, vertically centered. Drawn when the cell has a
        // `list`-type validator. Skipped if a note marker is already in the
        // top-right (they would collide).
        if renderer.data.get_note(row, col).is_none()
            && renderer
                .data
                .validations
                .get(row, col)
                .map(|v| v.validator.type_ == "list")
                .unwrap_or(false)
        {
            let x1 = draw_rect.x + draw_rect.width;
            let y_mid = draw_rect.y + draw_rect.height / 2f64;
            canvas
                .set_fill_style("#5a5a5a")
                .begin_path()
                .move_to(x1 - 10f64, y_mid - 3f64)
                .line_to(x1 - 2f64, y_mid - 3f64)
                .line_to(x1 - 6f64, y_mid + 3f64)
                .close_path()
                .fill(None);
        }

        // AutoFilter header glyph (issue #10): a small ▼ at the right edge of
        // each header cell in the active filter range — blue when that column
        // has an active value filter, gray otherwise.
        if renderer
            .data
            .auto_filter
            .hrange()
            .map(|h| h.includes(row, col))
            .unwrap_or(false)
        {
            let filtered = renderer
                .data
                .auto_filter
                .get_filter(col)
                .map(|f| f.operator == "in")
                .unwrap_or(false);
            let x1 = draw_rect.x + draw_rect.width;
            let y_mid = draw_rect.y + draw_rect.height / 2f64;
            canvas
                .set_fill_style(if filtered { "#1a73e8" } else { "#5a5a5a" })
                .begin_path()
                .move_to(x1 - 12f64, y_mid - 3f64)
                .line_to(x1 - 4f64, y_mid - 3f64)
                .line_to(x1 - 8f64, y_mid + 3f64)
                .close_path()
                .fill(None);
        }
    });
}

/// Draw one border edge if present. The tuple is (style, color); the style
/// name selects the line width.
fn draw_border_side(canvas: &Canvas, side: &Option<(String, String)>, x0: f64, y0: f64, x1: f64, y1: f64) {
    if let Some((style, color)) = side {
        let w = match style.as_str() {
            "medium" => 2.0,
            "thick" => 3.0,
            _ => 1.0, // thin / unknown
        };
        canvas.set_stroke_style(color.as_str());
        canvas.set_line_width(w);
        canvas.line(x0, y0, x1, y1);
    }
}

fn is_white(color: &str) -> bool {
    let c = color.trim().to_lowercase();
    c == "#fff" || c == "#ffffff" || c == "white"
}

/// Break `text` into lines that fit within `max_width`, breaking on spaces
/// (and honoring explicit newlines). The canvas font must already be set.
fn wrap_text(canvas: &Canvas, text: &str, max_width: f64) -> Vec<String> {
    let mut lines = Vec::new();
    for raw_line in text.split('\n') {
        let mut cur = String::new();
        for word in raw_line.split(' ') {
            let trial = if cur.is_empty() {
                word.to_string()
            } else {
                format!("{} {}", cur, word)
            };
            if !cur.is_empty() && canvas.measure_text_width(&trial) > max_width {
                lines.push(std::mem::take(&mut cur));
                cur = word.to_string();
            } else {
                cur = trial;
            }
        }
        lines.push(cur);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

/// Render the scrollable body area (background + cells), offsetting the canvas
/// to the area's on-screen position.
pub fn render_body(canvas: &Canvas, area: &Area, renderer: &TableRenderer) {
    canvas.save();
    canvas.translate(area.x, area.y);
    canvas.begin_path();
    canvas.rect(0f64, 0f64, area.width, area.height);
    canvas.clip(None);
    canvas.set_fill_style(renderer.bgcolor.as_str());
    canvas.fill_rect(0f64, 0f64, area.width, area.height);
    render_cells(canvas, area, renderer);
    canvas.restore();
}

/// Render the column header strip (A, B, C, …) aligned above the body area.
pub fn render_col_headers(canvas: &Canvas, area: &Area, renderer: &TableRenderer) {
    let h = renderer.col_header.height;
    if h <= 0f64 { return; }
    let hs = &renderer.header_style;
    let font = format!("{}px {}", hs.font_size, hs.font_family);

    canvas.save();
    canvas.translate(area.x, 0f64);
    canvas.begin_path();
    canvas.rect(0f64, 0f64, area.width, h);
    canvas.clip(None);
    if let Some(bg) = hs.bgcolor.clone() {
        canvas.set_fill_style(bg.as_str());
        canvas.fill_rect(0f64, 0f64, area.width, h);
    }
    canvas.set_line_width(renderer.header_gridline.width);
    area.each_col(|col, x, width| {
        canvas
            .set_font(&font)
            .set_fill_style(hs.color.as_str())
            .set_text_align("center")
            .set_text_baseline("middle");
        canvas.fill_text(&string_at(col), x + width / 2f64, h / 2f64, None);
        canvas.set_stroke_style(renderer.header_gridline.color.as_str());
        canvas.line(x + width, 0f64, x + width, h);
    });
    canvas.set_stroke_style(renderer.header_gridline.color.as_str());
    canvas.line(0f64, h, area.width, h);
    canvas.restore();
}

/// Render the row header gutter (1, 2, 3, …) aligned left of the body area.
pub fn render_row_headers(canvas: &Canvas, area: &Area, renderer: &TableRenderer) {
    let w = renderer.row_header.width;
    if w <= 0f64 { return; }
    let hs = &renderer.header_style;
    let font = format!("{}px {}", hs.font_size, hs.font_family);

    canvas.save();
    canvas.translate(0f64, area.y);
    canvas.begin_path();
    canvas.rect(0f64, 0f64, w, area.height);
    canvas.clip(None);
    if let Some(bg) = hs.bgcolor.clone() {
        canvas.set_fill_style(bg.as_str());
        canvas.fill_rect(0f64, 0f64, w, area.height);
    }
    canvas.set_line_width(renderer.header_gridline.width);
    area.each_row(|row, y, height| {
        canvas
            .set_font(&font)
            .set_fill_style(hs.color.as_str())
            .set_text_align("center")
            .set_text_baseline("middle");
        canvas.fill_text(&format!("{}", row + 1), w / 2f64, y + height / 2f64, None);
        canvas.set_stroke_style(renderer.header_gridline.color.as_str());
        canvas.line(0f64, y + height, w, y + height);
    });
    canvas.set_stroke_style(renderer.header_gridline.color.as_str());
    canvas.line(w, 0f64, w, area.height);
    canvas.restore();
}

/// Render the top-left corner box where the headers meet.
pub fn render_corner(canvas: &Canvas, renderer: &TableRenderer) {
    let w = renderer.row_header.width;
    let h = renderer.col_header.height;
    if w <= 0f64 || h <= 0f64 { return; }
    canvas.save();
    if let Some(bg) = renderer.header_style.bgcolor.clone() {
        canvas.set_fill_style(bg.as_str());
        canvas.fill_rect(0f64, 0f64, w, h);
    }
    canvas.set_line_width(renderer.header_gridline.width);
    canvas.set_stroke_style(renderer.header_gridline.color.as_str());
    canvas.line(w, 0f64, w, h);
    canvas.line(0f64, h, w, h);
    canvas.restore();
}

pub fn render_area(placement: Placement, canvas: &Canvas, area: &Area, renderer: &TableRenderer) {
    let _style = renderer.style.clone();
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
    let _area_merge_render_params: Vec<(Cell, Rect, Style)> = vec![];
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

        // Clear the whole canvas first.
        canvas.set_fill_style(renderer.bgcolor.as_str());
        canvas.fill_rect(0f64, 0f64, width, height);

        let area1 = viewport.areas.get(0).unwrap();
        let area2 = viewport.areas.get(1).unwrap();
        let area3 = viewport.areas.get(2).unwrap();
        let area4 = viewport.areas.get(3).unwrap();

        let (frow, fcol) = renderer.freeze;

        // Body areas. With no freeze only area4 is non-empty; frozen panes are
        // drawn on top so they stay fixed while the body scrolls.
        render_body(&canvas, area4, renderer);
        if frow > 0 { render_body(&canvas, area1, renderer); }
        if fcol > 0 { render_body(&canvas, area3, renderer); }
        if frow > 0 && fcol > 0 { render_body(&canvas, area2, renderer); }

        // Column headers span the frozen + body columns; row headers span the
        // frozen + body rows. area4 covers the common (no-freeze) case.
        render_col_headers(&canvas, area4, renderer);
        render_row_headers(&canvas, area4, renderer);
        if frow > 0 { render_row_headers(&canvas, area1, renderer); }
        if fcol > 0 { render_col_headers(&canvas, area3, renderer); }
        render_corner(&canvas, renderer);

        // Freeze divider lines.
        if frow > 0 || fcol > 0 {
            render_lines(&canvas, &renderer.freeze_gridline, || {
                if fcol > 0 { canvas.line(area4.x, 0.0, area4.x, height); }
                if frow > 0 { canvas.line(0.0, area4.y, width, area4.y); }
            });
        }

        // Selection rectangle.
        let selector = renderer.get_selector();
        render_selector(&canvas, &selector, viewport, renderer);

        // Scrollbars (only when content exceeds the viewport).
        let total_row_height: f64 = (0..renderer.data.row_count()).map(|i| renderer.row_height_at(i)).sum();
        let total_col_width: f64 = (0..renderer.data.col_count()).map(|i| renderer.col_width_at(i)).sum();

        let scrollbar_size = 12f64;
        let has_vertical_scrollbar = total_row_height > height - renderer.col_header.height;
        let has_horizontal_scrollbar = total_col_width > width - renderer.row_header.width;

        if has_vertical_scrollbar {
            let bar_x = width - scrollbar_size;
            let bar_y = renderer.col_header.height;
            let bar_height = height - renderer.col_header.height - if has_horizontal_scrollbar { scrollbar_size } else { 0f64 };
            let ratio = bar_height / total_row_height;
            let thumb_height = (bar_height * ratio).max(20f64).min(bar_height);
            let thumb_y = renderer.scroll_rows as f64 / renderer.data.row_count().max(1) as f64 * (bar_height - thumb_height);
            render_scrollbar(&canvas, bar_x, bar_y, scrollbar_size, bar_height, thumb_y, thumb_height, true);
        }

        if has_horizontal_scrollbar {
            let bar_x = renderer.row_header.width;
            let bar_y = height - scrollbar_size;
            let bar_width = width - renderer.row_header.width - if has_vertical_scrollbar { scrollbar_size } else { 0f64 };
            let ratio = bar_width / total_col_width;
            let thumb_width = (bar_width * ratio).max(20f64).min(bar_width);
            let thumb_x = renderer.scroll_cols as f64 / renderer.data.col_count().max(1) as f64 * (bar_width - thumb_width);
            render_scrollbar(&canvas, bar_x, bar_y, bar_width, scrollbar_size, thumb_x, thumb_width, false);
        }
    }
}