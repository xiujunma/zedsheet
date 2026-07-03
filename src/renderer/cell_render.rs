#![allow(dead_code)]

use crate::renderer::canvas::Canvas;
use crate::renderer::table_renderer::{
    Align, BorderLine, Cell, Rect, Style, TextLineType, VerticalAlign,
};
use regex::Regex;
use std::f64::consts::PI;

use super::table_renderer::BorderLineStyle;

#[derive(Debug, Clone)]
pub struct TextLine {
    pub width: f64,
    pub length: usize,
    pub start: usize,
}

pub fn text_x(align: &Align, width: f64, padding: f64) -> f64 {
    match align {
        Align::Left => padding,
        Align::Center => width / 2_f64,
        Align::Right => width - padding,
    }
}

pub fn text_y(
    align: VerticalAlign,
    height: f64,
    text_height: f64,
    font_height: f64,
    padding: f64,
) -> f64 {
    match align {
        VerticalAlign::Top => padding,
        VerticalAlign::Middle => {
            let y = height / 2_f64 - font_height / 2_f64;
            let min_height = font_height / 2_f64 + padding;
            if y < min_height {
                min_height
            } else {
                y
            }
        }
        VerticalAlign::Bottom => height - padding - text_height,
    }
}

pub fn text_line(
    text_line_type: TextLineType,
    align: Align,
    vertical_align: VerticalAlign,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) -> (f64, f64, f64, f64) {
    let mut ty = 0f64;
    if text_line_type == TextLineType::Underline {
        if vertical_align == VerticalAlign::Top {
            ty = -h
        } else if vertical_align == VerticalAlign::Middle {
            ty = -h / 2f64
        }
    } else if text_line_type == TextLineType::StrikeThrough {
        if vertical_align == VerticalAlign::Top {
            ty = -h / 2f64
        } else if vertical_align == VerticalAlign::Middle {
            ty = h / 2f64
        }
    }

    let mut tx = 0f64;

    if align == Align::Center {
        tx = w / 2_f64
    } else if align == Align::Right {
        tx = w
    }

    (x - tx, y - ty, x - tx + w, y - ty)
}

pub fn font_string(family: &str, size: f64, italic: bool, bold: bool) -> String {
    let mut font = String::from("");
    if italic {
        font.push_str("italic ");
    }

    if bold {
        font.push_str("bold ");
    }

    format!("{} {}px {}", font, size, family)
}

pub fn cell_border_render(
    canvas: &Canvas,
    rect: &Rect,
    border_line: &BorderLine,
    auto_align: Option<bool>,
) {
    canvas.save().begin_path().translate(rect.x, rect.y);

    let line_rects = |index: usize, offset: f64| -> (f64, f64, f64, f64) {
        let array = [
            (0f64 - offset, 0f64, rect.width + offset, 0f64),
            (rect.width, 0f64, rect.width, rect.height),
            (0f64 - offset, rect.height, rect.width + offset, rect.height),
            (0f64, 0f64, 0f64, rect.height),
        ];
        array[index]
    };

    let directions = vec![
        border_line.top.clone(),
        border_line.right.clone(),
        border_line.bottom.clone(),
        border_line.left.clone(),
    ];

    for (i, it) in directions.into_iter().enumerate() {
        if let Some(border) = it {
            let mut offset = 0f64;

            if auto_align.unwrap_or(false) {
                offset = line_params(border.0).0 / 2f64;
            }

            let rects = line_rects(i, offset);
            // Double draws two parallel lines instead of one.
            if border.0 == BorderLineStyle::Double {
                let (w, dash) = line_params(BorderLineStyle::Thin);
                canvas
                    .set_stroke_style(border.1.as_str())
                    .set_line_width(w)
                    .set_line_dash(&dash);
                let gap = 2.0_f64;
                match i {
                    0 => {
                        // top: draw a hair above and below
                        canvas.line(rects.0, 0.0 - gap / 2.0, rects.2, 0.0 - gap / 2.0);
                        canvas.line(rects.0, 0.0 + gap / 2.0, rects.2, 0.0 + gap / 2.0);
                    }
                    1 => {
                        // right
                        canvas.line(
                            rect.width - gap / 2.0,
                            rects.1,
                            rect.width - gap / 2.0,
                            rects.3,
                        );
                        canvas.line(
                            rect.width + gap / 2.0,
                            rects.1,
                            rect.width + gap / 2.0,
                            rects.3,
                        );
                    }
                    2 => {
                        // bottom
                        canvas.line(
                            rects.0,
                            rect.height - gap / 2.0,
                            rects.2,
                            rect.height - gap / 2.0,
                        );
                        canvas.line(
                            rects.0,
                            rect.height + gap / 2.0,
                            rects.2,
                            rect.height + gap / 2.0,
                        );
                    }
                    _ => {
                        // left
                        canvas.line(0.0 - gap / 2.0, rects.1, 0.0 - gap / 2.0, rects.3);
                        canvas.line(0.0 + gap / 2.0, rects.1, 0.0 + gap / 2.0, rects.3);
                    }
                }
            } else {
                let (line_width, line_dash) = line_params(border.0);
                canvas
                    .set_stroke_style(border.1.as_str())
                    .set_line_width(line_width)
                    .set_line_dash(&line_dash)
                    .line(rects.0, rects.1, rects.2, rects.3);
            }
        }
    }

    // Diagonal lines run from corner to corner regardless of the four
    // edges. `up` = top-left → bottom-right; `down` = bottom-left → top-right.
    if let Some(border) = &border_line.diagonal_up {
        let (w, dash) = line_params(border.0);
        canvas
            .set_stroke_style(border.1.as_str())
            .set_line_width(w)
            .set_line_dash(&dash)
            .line(0.0, 0.0, rect.width, rect.height);
    }
    if let Some(border) = &border_line.diagonal_down {
        let (w, dash) = line_params(border.0);
        canvas
            .set_stroke_style(border.1.as_str())
            .set_line_width(w)
            .set_line_dash(&dash)
            .line(0.0, rect.height, rect.width, 0.0);
    }

    canvas.restore();
}

/// Map a [`BorderLineStyle`] to (line_width, line_dash). The dashed and
/// dotted styles are tiny: 1-2px gaps that read as such at default zoom.
fn line_params(style: BorderLineStyle) -> (f64, Vec<f64>) {
    match style {
        BorderLineStyle::Thin => (1.0, vec![]),
        BorderLineStyle::Medium => (2.0, vec![]),
        BorderLineStyle::Thick => (3.0, vec![]),
        BorderLineStyle::Dotted => (1.0, vec![1.0, 1.0]),
        BorderLineStyle::Dashed => (1.0, vec![2.0, 2.0]),
        // Double is handled by the caller drawing two parallel lines.
        BorderLineStyle::Double => (1.0, vec![]),
    }
}

pub fn cell_render<R, F>(
    canvas: &Canvas,
    cell: &Cell,
    rect: &Rect,
    style: &Style,
    cell_renderer: R,
    formatter: F,
) where
    R: Fn(&Canvas, &Rect, &Cell, &str) -> bool + 'static,
    F: Fn(&Cell) -> String + 'static,
{
    let text = formatter(cell);

    canvas.save().begin_path().translate(rect.x, rect.y);

    canvas.rect(0f64, 0f64, rect.width, rect.height).clip(None);

    if let Some(bgcolor) = &style.bgcolor {
        canvas.set_fill_style(bgcolor);
    }

    if let Some(rotation) = style.rotation {
        canvas.rotate(rotation * (PI / 180_f64));
    }

    canvas.save();
    if !cell_renderer(canvas, rect, cell, text.as_str()) {
        canvas.restore();
        return;
    }
    canvas.restore();

    //
    let re = Regex::new(r"^\s*$").unwrap();
    if re.is_match(text.as_str()) {
        canvas
            .save()
            .begin_path()
            .set_text_align(style.align.to_string().as_str())
            .set_text_baseline(style.valign.to_string().as_str())
            .set_font(
                font_string(
                    &style.font_family,
                    style.font_size as f64,
                    style.italic,
                    style.bold,
                )
                .as_str(),
            )
            .set_fill_style(style.color.as_str());

        let (xp, yp) = style.padding.unwrap_or((5f64, 5f64));

        let tx = text_x(&style.align, rect.width, xp);
        let txts = text.split("\n");
        let inner_width = rect.width - xp * 2_f64;
        let mut ntxts = vec![];

        for it in txts {
            let txt_width = canvas.measure_text_width(it);

            if style.text_wrap && txt_width > inner_width {
                let mut text_line = TextLine {
                    width: 0f64,
                    length: 0,
                    start: 0,
                };

                for i in 0..it.len() {
                    if text_line.width > inner_width {
                        ntxts.push(&it[text_line.start..text_line.length]);
                        text_line = TextLine {
                            width: 0f64,
                            length: 0,
                            start: i,
                        }
                    }
                    text_line.length += 1;
                    text_line.width += canvas.measure_text_width(&it[i..i + 1]) + 1f64;
                }

                if text_line.length > 0 {
                    ntxts.push(&it[text_line.start..text_line.length]);
                }
            } else {
                ntxts.push(it);
            }
        }

        let font_height = style.font_size as f64 / 0.75;
        let text_height = font_height * (ntxts.len() - 1) as f64;
        let mut line_types = vec![];

        if style.underline {
            line_types.push(TextLineType::Underline);
        }

        if style.strike_through {
            line_types.push(TextLineType::StrikeThrough);
        }

        let ty = text_y(
            style.valign.clone(),
            rect.height,
            text_height,
            font_height,
            yp,
        );

        for it in ntxts {
            let text_width = canvas.measure_text_width(it);
            canvas.fill_text(it, tx, ty, None);
            for line_type in line_types.clone() {
                let (x1, y1, x2, y2) = text_line(
                    line_type,
                    style.align.clone(),
                    style.valign.clone(),
                    tx,
                    ty,
                    text_width,
                    font_height,
                );
                canvas.line(x1, y1, x2, y2);
            }
        }
        canvas.restore();
    }

    canvas.restore();
}
