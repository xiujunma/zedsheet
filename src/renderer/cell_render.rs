#![allow(dead_code)]

use std::f64::consts::PI;
use regex::Regex;
use crate::renderer::canvas::Canvas;
use crate::renderer::table_renderer::{Align, BorderLine, Cell, CellRenderer, Formatter, Rect, Style, TextLineType, VerticalAlign};


struct TextLine {
    pub width: f64,
    pub length: usize,
    pub start: usize
}

pub fn text_x(align: &Align, width: f64, padding: f64) -> f64 {
    return match align {
        Align::Left => {
            padding
        },
        Align::Center => {
            width / 2_f64
        },
        Align::Right => {
            width - padding
        }
    }
}

pub fn text_y(align: VerticalAlign, height: f64, text_height: f64, font_height: f64, padding: f64) -> f64 {
    return match align {
        VerticalAlign::Top => {
            padding
        },
        VerticalAlign::Middle => {
            let y = height / 2_f64 - font_height / 2_f64;
            let min_height = font_height / 2 + padding;
            if y < min_height {
                min_height
            } else {
                y
            }
        },
        VerticalAlign::Bottom => {
            height - padding - text_height
        }
    }
}

pub fn text_line(text_line_type: TextLineType, align: Align, vertical_align: VerticalAlign, x: f64, y: f64, w: f64, h: f64) -> (f64, f64, f64, f64) {
    let mut ty = 0f64;
    if text_line_type == TextLineType::Underline {
        if vertical_align == VerticalAlign::Top {
            ty = -h
        } else if vertical_align == VerticalAlign::Middle {
            ty = -h / 2
        }
    } else if text_line_type == TextLineType::StrikeThrough {
        if vertical_align == VerticalAlign::Top {
            ty = -h / 2
        } else if vertical_align == VerticalAlign::Middle {
            ty = h / 2
        }
    }

    let mut tx = 0f64;

    if align == Align::Center {
        tx = w / 2_f64
    } else if align == Align::Right {
        tx = w
    }

    return (x - tx, y - ty, x - tx + w, y - ty)
}

pub fn font_string(family: &str, size: f64, italic: bool, bold: bool) -> String {
    let mut font = "";
    if italic {
        font.push_str("italic ");
    }

    if bold {
        font.push_str("bold ");
    }

    format!("{} {}px {}", font, size, family)
}

pub fn cell_border_render(canvas: Canvas, rect: Rect, border_line: BorderLine, auto_align: bool) {

}

pub fn cell_render(canvas: Canvas, cell: Cell, rect: Rect, style: Style, cell_renderer: Option<CellRenderer>, formatter: Formatter) {
    let text = formatter.format(cell.clone());

    canvas.save().begin_path().translate(rect.x, rect.y);

    canvas.rect(0f64, 0f64, rect.width, rect.height).clip(None);

    match style.bgcolor {
        Some(bgcolor) => {
            canvas.prop("fillStyle", bgcolor);
        },
        _ => {}
    }

    match style.rotation {
        Some(rotation) => {
            canvas.rotate(rotation * ( PI / 180_f64));

        },
        _ => {}
    }

    match cell_renderer {
        Some(cell_renderer) => {
            canvas.save();
            if !cell_renderer.render(canvas, rect, cell, text.clone()) {
                canvas.restore();
                return;
            }
            canvas.restore();
        },
        _ => {}
    }

    //
    let re = Regex::new(r"^\s*$").unwrap();
    if re.test(text.clone()) {
        canvas
            .save()
            .beginPath()
            .prop("textAlign", style.align)
            .prop("textBaseline", style.valign)
            .prop("font", font_string(&style.font_family, style.font_size as f64, style.italic, style.bold))
            .prop("fillStyle", style.color);

        let (xp, yp) = match style.padding {
            Some(padding) => {
                padding
            },
            _ => {
                (5f64, 5f64)
            }
        };

        let tx = text_x(&style.align, rect.width.clone(), xp);
        let txts = text.split("\n");
        let inner_width = &rect.width - xp * 2_f64;
        let mut ntxts = vec![];

        for it in txts {
            let txt_width = canvas.measure_text_width(it);

            if style.text_wrap && txt_width > inner_width {
                let mut text_line = TextLine {
                    width: 0f64,
                    length: 0,
                    start: 0
                };

                for i in 0..it.len() {
                    if text_line.width > inner_width {
                        ntxts.push(it.substring(text_line.start, text_line.length));
                        text_line = TextLine {
                            width: 0f64,
                            length: 0,
                            start: i
                        }
                    }
                    text_line.length += 1;
                    text_line.width += canvas.measure_text_width(it.substring(i, i + 1)) + 1;
                }

                if text_line.length > 0 {
                    ntxts.push(it.substring(text_line.start, text_line.length));
                }
            } else {
                ntxts.push(it);
            }
        }

        let font_height = style.font_size as f64 / 0.75;
        let text_height = font_height * (ntxts.len() - 1f64);
        let mut line_types = vec![];

        if style.underline {
            line_types.push(TextLineType::Underline);
        }

        if style.strike_through {
            line_types.push(TextLineType::StrikeThrough);
        }

        let ty = text_y(style.valign.clone(), rect.height.clone(), text_height, font_height, yp);

        for it in ntxts {
            let text_width = canvas.measure_text_width(it);
            canvas.fill_text(it, tx, ty, None);
            for line_type in line_types.clone() {
                let (x1, y1, x2, y2) = text_line(line_type, style.align.clone(), style.valign.clone(), tx, ty, text_width, font_height);
                canvas.line(x1, y1, x2, y2);
            }
        }
        canvas.restore();
    }

    canvas.restore();
}