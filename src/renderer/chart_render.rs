//! Canvas drawing for charts (issue #16). Charts render as the last body
//! layer, anchored at a cell, so they float over the grid, scroll with it,
//! and re-read their data range every render (live updates). Pure geometry +
//! the existing `Canvas` wrapper — no DOM.

use crate::core::chart::{extract_chart_data, nice_ceil, ChartData};
use crate::renderer::alphabets::exp2xy;
use crate::renderer::canvas::Canvas;
use crate::renderer::table_renderer::TableRenderer;

const PALETTE: [&str; 8] = [
    "#1e88e5", "#e53935", "#43a047", "#fb8c00", "#8e24aa", "#00897b", "#3949ab", "#d81b60",
];

/// Draw every chart whose anchor cell is inside the current viewport.
pub fn draw_charts(canvas: &Canvas, renderer: &TableRenderer) {
    for chart in &renderer.data.charts {
        let (ac, ar) = exp2xy(&chart.anchor);
        // Anchors scrolled off the top/left can't be positioned; they reappear
        // when scrolled back into view.
        if ar < renderer.body_start_row() || ac < renderer.body_start_col() {
            continue;
        }
        let rect = renderer.cell_screen_rect(ar, ac);
        if rect.x >= renderer.width || rect.y >= renderer.height {
            continue; // fully off-screen to the right/bottom
        }
        let (x, y, w, h) = (rect.x, rect.y, chart.width, chart.height);

        // Panel.
        canvas.save();
        canvas.set_fill_style("#ffffff");
        canvas.fill_rect(x, y, w, h);
        canvas.set_stroke_style("#b0b6bd");
        canvas.set_line_width(1.0);
        canvas.stroke_rect(x + 0.5, y + 0.5, w - 1.0, h - 1.0);

        // Title.
        let title = if chart.title.is_empty() {
            &chart.range
        } else {
            &chart.title
        };
        canvas.set_fill_style("#333333");
        canvas.set_font("bold 12px Arial");
        canvas.set_text_align("center");
        canvas.fill_text(title, x + w / 2.0, y + 16.0, Some(w - 8.0));

        // Plot box.
        let (px, py) = (x + 42.0, y + 26.0);
        let (pw, ph) = ((w - 52.0).max(20.0), (h - 54.0).max(20.0));

        match extract_chart_data(&renderer.data, &chart.range) {
            Some(data) if !data.labels.is_empty() => match chart.kind.as_str() {
                "line" => draw_axes_chart(canvas, px, py, pw, ph, &data, false),
                "pie" => draw_pie(canvas, px, py, pw, ph, &data),
                _ => draw_axes_chart(canvas, px, py, pw, ph, &data, true),
            },
            _ => {
                canvas.set_font("11px Arial");
                canvas.set_fill_style("#999999");
                canvas.fill_text("No data", x + w / 2.0, y + h / 2.0, None);
            }
        }
        canvas.set_text_align("left");
        canvas.restore();
    }
}

/// Shared bar/line body: y axis with nice bounds + gridlines, x labels,
/// a legend when there are multiple series.
fn draw_axes_chart(
    canvas: &Canvas,
    px: f64,
    py: f64,
    pw: f64,
    ph: f64,
    data: &ChartData,
    bars: bool,
) {
    let all: Vec<f64> = data
        .series
        .iter()
        .flat_map(|(_, v)| v.iter().copied())
        .collect();
    let raw_max = all
        .iter()
        .cloned()
        .fold(f64::NEG_INFINITY, f64::max)
        .max(0.0);
    let raw_min = all.iter().cloned().fold(f64::INFINITY, f64::min).min(0.0);
    let ymax = nice_ceil(raw_max);
    let ymin = if raw_min < 0.0 {
        -nice_ceil(-raw_min)
    } else {
        0.0
    };
    let span = (ymax - ymin).max(1e-9);
    let y_of = |v: f64| py + ph - (v - ymin) / span * ph;

    // Gridlines + y labels.
    canvas.set_font("10px Arial");
    canvas.set_text_align("right");
    let ticks = 4;
    for i in 0..=ticks {
        let v = ymin + span * i as f64 / ticks as f64;
        let gy = y_of(v);
        canvas.set_stroke_style("#e4e7ea");
        canvas.line(px, gy, px + pw, gy);
        canvas.set_fill_style("#777777");
        canvas.fill_text(&format_tick(v), px - 5.0, gy + 3.0, None);
    }

    let n = data.labels.len();
    let group_w = pw / n as f64;

    // X labels.
    canvas.set_text_align("center");
    for (i, label) in data.labels.iter().enumerate() {
        let cx = px + group_w * (i as f64 + 0.5);
        canvas.set_fill_style("#777777");
        canvas.fill_text(label, cx, py + ph + 14.0, Some(group_w - 2.0));
    }

    // Zero line.
    canvas.set_stroke_style("#9aa0a6");
    canvas.line(px, y_of(0.0), px + pw, y_of(0.0));

    if bars {
        let slot = group_w * 0.8 / data.series.len() as f64;
        for (si, (_, values)) in data.series.iter().enumerate() {
            canvas.set_fill_style(PALETTE[si % PALETTE.len()]);
            for (i, v) in values.iter().enumerate() {
                let bx = px + group_w * i as f64 + group_w * 0.1 + slot * si as f64;
                let (top, bottom) = if *v >= 0.0 {
                    (y_of(*v), y_of(0.0))
                } else {
                    (y_of(0.0), y_of(*v))
                };
                canvas.fill_rect(bx, top, (slot - 2.0).max(1.0), (bottom - top).max(1.0));
            }
        }
    } else {
        for (si, (_, values)) in data.series.iter().enumerate() {
            let color = PALETTE[si % PALETTE.len()];
            canvas.set_stroke_style(color);
            canvas.set_line_width(2.0);
            canvas.begin_path();
            for (i, v) in values.iter().enumerate() {
                let cx = px + group_w * (i as f64 + 0.5);
                let cy = y_of(*v);
                if i == 0 {
                    canvas.move_to(cx, cy);
                } else {
                    canvas.line_to(cx, cy);
                }
            }
            canvas.stroke();
            canvas.set_fill_style(color);
            for (i, v) in values.iter().enumerate() {
                let cx = px + group_w * (i as f64 + 0.5);
                canvas.fill_rect(cx - 2.0, y_of(*v) - 2.0, 4.0, 4.0);
            }
        }
    }

    // Legend (only when it disambiguates).
    if data.series.len() > 1 {
        canvas.set_font("10px Arial");
        canvas.set_text_align("left");
        let mut lx = px + 4.0;
        let ly = py - 4.0;
        for (si, (name, _)) in data.series.iter().enumerate() {
            canvas.set_fill_style(PALETTE[si % PALETTE.len()]);
            canvas.fill_rect(lx, ly - 7.0, 8.0, 8.0);
            canvas.set_fill_style("#555555");
            canvas.fill_text(name, lx + 11.0, ly, None);
            lx += 11.0 + canvas.measure_text_width(name) + 12.0;
        }
    }
}

/// Pie of the first series, with a label + percentage legend on the right.
fn draw_pie(canvas: &Canvas, px: f64, py: f64, pw: f64, ph: f64, data: &ChartData) {
    let Some((_, values)) = data.series.first() else {
        return;
    };
    let total: f64 = values.iter().map(|v| v.abs()).sum();
    if total <= 0.0 {
        return;
    }
    let legend_w = (pw * 0.38).min(120.0);
    let r = ((pw - legend_w).min(ph) / 2.0 - 4.0).max(8.0);
    let (cx, cy) = (px + (pw - legend_w) / 2.0, py + ph / 2.0);

    let mut angle = -std::f64::consts::FRAC_PI_2;
    for (i, v) in values.iter().enumerate() {
        let sweep = v.abs() / total * std::f64::consts::TAU;
        canvas.set_fill_style(PALETTE[i % PALETTE.len()]);
        canvas.begin_path();
        canvas.move_to(cx, cy);
        canvas.arc(cx, cy, r, angle, angle + sweep, None);
        canvas.close_path();
        canvas.fill(None);
        angle += sweep;
    }

    canvas.set_font("10px Arial");
    canvas.set_text_align("left");
    let mut ly = py + 10.0;
    for (i, v) in values.iter().enumerate() {
        let pct = v.abs() / total * 100.0;
        let label = data.labels.get(i).cloned().unwrap_or_default();
        canvas.set_fill_style(PALETTE[i % PALETTE.len()]);
        canvas.fill_rect(px + pw - legend_w, ly - 7.0, 8.0, 8.0);
        canvas.set_fill_style("#555555");
        canvas.fill_text(
            &format!("{} ({:.0}%)", label, pct),
            px + pw - legend_w + 11.0,
            ly,
            Some(legend_w - 12.0),
        );
        ly += 14.0;
        if ly > py + ph {
            break;
        }
    }
}

fn format_tick(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        format!("{:.1}", v)
    }
}
