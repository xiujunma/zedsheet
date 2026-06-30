//! Canvas drawing for charts (issue #16). Charts render as the last body
//! layer, anchored at a cell, so they float over the grid, scroll with it,
//! and re-read their data range every render (live updates). Pure geometry +
//! the existing `Canvas` wrapper — no DOM.

use crate::core::chart::{extract_chart_data, extract_secondary_chart_data, nice_ceil, ChartData};
use crate::core::sparkline::{Sparkline, SparklineKind};
use crate::core::trendline::{
    exponential_eval, exponential_regression, linear_eval, linear_regression, quadratic_eval,
    quadratic_regression, Trendline,
};
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
            Some(data) if !data.labels.is_empty() => {
                // Secondary axis (Phase 2.2b): when `secondary_range`
                // is set AND its extracted data is non-empty, draw a
                // dual-axis combo chart. The primary range's series
                // render as bars at the left-axis scale; the
                // secondary range's series render as a line overlay
                // at the right-axis scale.
                let combo = chart
                    .secondary_range
                    .as_deref()
                    .and_then(|sr| extract_secondary_chart_data(&renderer.data, sr, &data.labels));
                if let Some(secondary) = combo {
                    draw_combo_chart(canvas, px, py, pw, ph, &data, &secondary, chart.trendline);
                } else {
                    match chart.kind.as_str() {
                        "line" => {
                            draw_axes_chart(canvas, px, py, pw, ph, &data, "line", chart.trendline)
                        }
                        "scatter" => draw_axes_chart(
                            canvas,
                            px,
                            py,
                            pw,
                            ph,
                            &data,
                            "scatter",
                            chart.trendline,
                        ),
                        "bubble" => draw_axes_chart(
                            canvas,
                            px,
                            py,
                            pw,
                            ph,
                            &data,
                            "bubble",
                            chart.trendline,
                        ),
                        "area" => {
                            draw_axes_chart(canvas, px, py, pw, ph, &data, "area", chart.trendline)
                        }
                        "radar" => draw_radar(canvas, px, py, pw, ph, &data, chart.trendline),
                        "doughnut" => draw_doughnut(canvas, px, py, pw, ph, &data),
                        "pie" => draw_pie(canvas, px, py, pw, ph, &data),
                        _ => draw_axes_chart(canvas, px, py, pw, ph, &data, "bar", chart.trendline),
                    }
                }
            }
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

/// Shared bar/line/scatter/bubble/area body: y axis with nice bounds +
/// gridlines, x labels, a legend when there are multiple series.
/// `mode` selects the per-series draw style:
///   - `"bar"`     → filled rectangles
///   - `"line"`    → connected line + dot markers
///   - `"scatter"` → dot markers only (no connecting line)
///   - `"bubble"`  → like scatter, with radius proportional to |value|
///   - `"area"`    → like line, but the area under the curve is filled
///
/// When `trendline` is set, one fitted curve is drawn over each series
/// after the body (Phase 1.2).
fn draw_axes_chart(
    canvas: &Canvas,
    px: f64,
    py: f64,
    pw: f64,
    ph: f64,
    data: &ChartData,
    mode: &str,
    trendline: Trendline,
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

    match mode {
        "bar" => {
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
        }
        "line" => {
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
        "scatter" => {
            // Plot only the data points, no connecting line. Same
            // axes as bar/line. Fixed 4px square dot per point.
            for (si, (_, values)) in data.series.iter().enumerate() {
                let color = PALETTE[si % PALETTE.len()];
                canvas.set_fill_style(color);
                for (i, v) in values.iter().enumerate() {
                    let cx = px + group_w * (i as f64 + 0.5);
                    let cy = y_of(*v);
                    canvas.fill_rect(cx - 2.0, cy - 2.0, 4.0, 4.0);
                }
            }
        }
        "bubble" => {
            // Like scatter, but with a circle radius proportional to
            // |value|, clamped to a legible range. Uses the canvas
            // `arc` helper if available; falls back to fill_rect when
            // the canvas wrapper doesn't expose one.
            let vmax = data
                .series
                .iter()
                .flat_map(|(_, v)| v.iter())
                .fold(0.0_f64, |m, v| m.max(v.abs()))
                .max(1e-9);
            for (si, (_, values)) in data.series.iter().enumerate() {
                let color = PALETTE[si % PALETTE.len()];
                canvas.set_fill_style(color);
                for (i, v) in values.iter().enumerate() {
                    let cx = px + group_w * (i as f64 + 0.5);
                    let cy = y_of(*v);
                    // Radius in [3, 14] CSS px, scaled to |v|/vmax.
                    let r = (3.0 + 11.0 * (v.abs() / vmax)).clamp(3.0, 14.0);
                    // Squares as a stand-in for circles — the canvas
                    // wrapper only exposes fill_rect; a future helper
                    // could add arc(). Half-side = r/2 keeps total
                    // size ≈ r for visual parity.
                    let half = r / 2.0;
                    canvas.fill_rect(cx - half, cy - half, r, r);
                }
            }
        }
        "area" => {
            // Like line, but the area under the curve is filled
            // with a translucent variant of the series colour. The
            // fill path runs along the line from the first x to the
            // last x, drops to the y=0 baseline at the right edge,
            // runs back along zero to the left edge, then closes.
            for (si, (_, values)) in data.series.iter().enumerate() {
                let color = PALETTE[si % PALETTE.len()];
                canvas.set_fill_style(&with_alpha(color, 0.25));
                canvas.begin_path();
                if let Some(v0) = values.first() {
                    let cx0 = px + group_w * 0.5;
                    canvas.move_to(cx0, y_of(*v0));
                    for (i, v) in values.iter().enumerate().skip(1) {
                        let cx = px + group_w * (i as f64 + 0.5);
                        canvas.line_to(cx, y_of(*v));
                    }
                }
                // Close down to the zero baseline at the right edge,
                // back along zero to the left edge, then up to the
                // starting point.
                if !values.is_empty() {
                    let last_cx = px + group_w * (values.len() as f64 - 0.5);
                    canvas.line_to(last_cx, y_of(0.0));
                    canvas.line_to(px + group_w * 0.5, y_of(0.0));
                }
                canvas.close_path();
                canvas.fill(None);
                // Then the line on top, opaque.
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
            }
        }
        _ => {
            // Unknown mode: fall back to a single neutral marker so a
            // bad string doesn't silently draw nothing.
            canvas.set_fill_style("#888888");
            for (i, _v) in data.series[0].1.iter().enumerate() {
                let cx = px + group_w * (i as f64 + 0.5);
                let cy = py + ph * 0.5;
                canvas.fill_rect(cx - 2.0, cy - 2.0, 4.0, 4.0);
            }
        }
    }

    // Trendline (Phase 1.2): one fitted curve over each series,
    // drawn after the series so it sits on top. Fitted values are
    // clamped to the visible y-range so the curve never leaves the
    // plot box; lines outside the box are not extrapolated.
    if trendline.is_visible() {
        let n_points = data.labels.len();
        if n_points >= 2 {
            for (si, (_, values)) in data.series.iter().enumerate() {
                let fitted: Option<Vec<f64>> = match trendline {
                    Trendline::Linear => linear_regression(values)
                        .map(|fit| (0..n_points).map(|i| linear_eval(fit, i)).collect()),
                    Trendline::Exponential => exponential_regression(values)
                        .map(|fit| (0..n_points).map(|i| exponential_eval(fit, i)).collect()),
                    Trendline::Polynomial => quadratic_regression(values)
                        .map(|fit| (0..n_points).map(|i| quadratic_eval(fit, i)).collect()),
                    Trendline::None => None,
                };
                if let Some(ys) = fitted {
                    // Trendline colour: dark variant of the series'
                    // PALETTE slot, falling back to near-black for the
                    // 8th slot.
                    let base = PALETTE[si % PALETTE.len()];
                    let dark = darken(base);
                    canvas.set_stroke_style(&dark);
                    canvas.set_line_width(1.5);
                    canvas.begin_path();
                    for (i, y_val) in ys.iter().enumerate() {
                        // Clamp to the visible y-range so the line
                        // doesn't escape the plot box.
                        let clamped = y_val.clamp(ymin, ymax);
                        let cx = px + group_w * (i as f64 + 0.5);
                        let cy = y_of(clamped);
                        if i == 0 {
                            canvas.move_to(cx, cy);
                        } else {
                            canvas.line_to(cx, cy);
                        }
                    }
                    canvas.stroke();
                }
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

/// Render every sparkline anchored to a visible cell (Phase 4.1b).
/// Each sparkline is painted as a tiny chart inside the cell's
/// screen rect with 4px padding all around. We re-read the data
/// range on every render so a live data update propagates to the
/// sparkline immediately.
pub fn draw_sparklines(canvas: &Canvas, renderer: &TableRenderer) {
    for sparkline in &renderer.data.sparklines {
        let (ac, ar) = match crate::renderer::alphabets::exp2xy(&sparkline.anchor) {
            (c, r) if r < renderer.body_start_row() || c < renderer.body_start_col() => continue,
            (c, r) => (c, r),
        };
        let rect = renderer.cell_screen_rect(ar, ac);
        if rect.x >= renderer.width || rect.y >= renderer.height {
            continue;
        }
        let data = match crate::core::chart::extract_chart_data(&renderer.data, &sparkline.range) {
            Some(d) if !d.series.is_empty() => d,
            _ => continue,
        };
        draw_one_sparkline(canvas, &rect, &sparkline, &data);
    }
}

fn draw_one_sparkline(
    canvas: &Canvas,
    rect: &crate::renderer::table_renderer::Rect,
    sparkline: &Sparkline,
    data: &ChartData,
) {
    let (x, y, w, h) = (rect.x, rect.y, rect.width, rect.height);
    // 4px padding all around so the line/bars don't touch the cell border.
    let pad = 4.0;
    let px = x + pad;
    let py = y + pad;
    let pw = (w - pad * 2.0).max(8.0);
    let ph = (h - pad * 2.0).max(8.0);
    let values: &[f64] = &data.series[0].1;
    if values.is_empty() {
        return;
    }
    let n = values.len();
    let vmin = values
        .iter()
        .cloned()
        .fold(f64::INFINITY, f64::min)
        .min(0.0);
    let vmax = values
        .iter()
        .cloned()
        .fold(f64::NEG_INFINITY, f64::max)
        .max(0.0);
    let span = (vmax - vmin).max(1e-9);
    let color = sparkline.effective_color();
    canvas.set_stroke_style(&color);
    canvas.set_fill_style(&color);
    canvas.set_line_width(1.0);
    let slot_w = pw / n as f64;
    match sparkline.kind {
        SparklineKind::Line => {
            canvas.begin_path();
            for (i, &v) in values.iter().enumerate() {
                let cx = px + slot_w * (i as f64 + 0.5);
                let cy = py + ph - (v - vmin) / span * ph;
                if i == 0 {
                    canvas.move_to(cx, cy);
                } else {
                    canvas.line_to(cx, cy);
                }
            }
            canvas.stroke();
            // Final-value dot so the line is clearly closed.
            let last = values.last().copied().unwrap_or(0.0);
            let cx = px + slot_w * (n as f64 - 0.5);
            let cy = py + ph - (last - vmin) / span * ph;
            canvas.fill_rect(cx - 2.0, cy - 2.0, 4.0, 4.0);
        }
        SparklineKind::Column => {
            let bar_w = (slot_w * 0.7).max(1.0);
            // Zero baseline at v=0 in the cell's coords.
            let baseline = py + ph - (0.0 - vmin) / span * ph;
            for (i, &v) in values.iter().enumerate() {
                let cx = px + slot_w * (i as f64 + 0.5) - bar_w / 2.0;
                let top = py + ph - (v.max(0.0) - vmin) / span * ph;
                let bottom = py + ph - (v.min(0.0) - vmin) / span * ph;
                let (rect_top, rect_h) = if top < bottom {
                    (top, bottom - top)
                } else {
                    (bottom, top - bottom)
                };
                canvas.fill_rect(cx, rect_top, bar_w, rect_h.max(1.0));
            }
            // Subtle zero line.
            canvas.set_stroke_style("#e4e7ea");
            canvas.begin_path();
            canvas.move_to(px, baseline);
            canvas.line_to(px + pw, baseline);
            canvas.stroke();
            canvas.set_stroke_style(&color);
        }
        SparklineKind::WinLoss => {
            let block_w = (slot_w * 0.9).max(1.0);
            canvas.set_stroke_style("#888888");
            for (i, &v) in values.iter().enumerate() {
                let cx = px + slot_w * (i as f64 + 0.5) - block_w / 2.0;
                let (top, bottom) = if v >= 0.0 {
                    (py + ph / 2.0, py + ph)
                } else {
                    (py, py + ph / 2.0)
                };
                canvas.set_fill_style(if v >= 0.0 { "#43a047" } else { "#e53935" });
                canvas.fill_rect(cx, top, block_w, bottom - top);
            }
            canvas.set_fill_style(&color);
            canvas.set_stroke_style(&color);
        }
    }
}

/// Render every floating image anchored to a visible cell (Phase 4.2).
/// Each image is loaded from its `src` URL once on the first frame
/// and cached in a thread-local keyed by URL; subsequent frames
/// re-blit the cached `HtmlImageElement` without re-fetching. The
/// cache itself lives in \`zedsheet::image_loader\`; this function
/// is just the "ask the loader to ensure the URL is fetched, then
/// blit" pass.
pub fn draw_images(canvas: &Canvas, renderer: &TableRenderer) {
    for image in &renderer.data.images {
        let (ac, ar) = match crate::renderer::alphabets::exp2xy(&image.anchor) {
            (c, r) if r < renderer.body_start_row() || c < renderer.body_start_col() => continue,
            (c, r) => (c, r),
        };
        let rect = renderer.cell_screen_rect(ar, ac);
        if rect.x >= renderer.width || rect.y >= renderer.height {
            continue;
        }
        let src = image.src.trim();
        if src.is_empty() {
            continue;
        }
        // Fire-and-forget: the first call kicks off the load,
        // subsequent calls are no-ops. The image appears in the
        // cache once \`onload\` fires.
        crate::zedsheet::image_loader::ensure_loaded(src);
        let Some(html_img) = crate::zedsheet::image_loader::get(src) else {
            // In-flight or previously-failed; skip this frame.
            continue;
        };
        // The image's anchor sits at the cell's top-left; width /
        // height come from the model (defaults to a 2×1-cell block).
        let w = image.width.max(1.0);
        let h = image.height.max(1.0);
        canvas.draw_image_html(&html_img, rect.x, rect.y, w, h);
    }
}

/// Darken a hex colour string (\"#rrggbb\") by halving each channel
/// (with a floor to keep the result legible). Used to give the
/// trendline a darker shade than its series. Pure helper, host-tested.
fn darken(hex: &str) -> String {
    let s = hex.strip_prefix('#').unwrap_or(hex);
    if s.len() != 6 {
        return "#333333".to_string();
    }
    let r = u8::from_str_radix(&s[0..2], 16).ok();
    let g = u8::from_str_radix(&s[2..4], 16).ok();
    let b = u8::from_str_radix(&s[4..6], 16).ok();
    match (r, g, b) {
        (Some(r), Some(g), Some(b)) => {
            let r = (r / 2 + 32).min(255);
            let g = (g / 2 + 32).min(255);
            let b = (b / 2 + 32).min(255);
            format!("#{:02x}{:02x}{:02x}", r, g, b)
        }
        _ => "#333333".to_string(),
    }
}

/// Convert a hex colour string (\"#rrggbb\") to an rgba() CSS string
/// at the given alpha (0..=1). Used by the area renderer to make a
/// translucent fill for the curve. Pure helper, host-tested.
fn with_alpha(hex: &str, alpha: f64) -> String {
    let s = hex.strip_prefix('#').unwrap_or(hex);
    if s.len() != 6 {
        return format!("rgba(128,128,128,{})", alpha.clamp(0.0, 1.0));
    }
    let r = u8::from_str_radix(&s[0..2], 16).ok();
    let g = u8::from_str_radix(&s[2..4], 16).ok();
    let b = u8::from_str_radix(&s[4..6], 16).ok();
    match (r, g, b) {
        (Some(r), Some(g), Some(b)) => {
            format!("rgba({},{},{},{})", r, g, b, alpha.clamp(0.0, 1.0))
        }
        _ => format!("rgba(128,128,128,{})", alpha.clamp(0.0, 1.0)),
    }
}

/// Dual-axis combination chart (Phase 2.2). The primary range's
/// series render as bars at the left-axis scale; the secondary
/// range's series render as a line overlay at the right-axis
/// scale. The two Y axes share the same X (the primary labels).
///
/// Layout adjustment: the plot box is shrunk a bit to leave room
/// for the right-side tick labels (≈ 22 extra px). The trendline
/// overlay is skipped to keep the dual-axis version simple — the
/// secondary line is the second axis already, and overlaying a
/// fitted curve on top of an axis that already has its own series
/// is busy enough without trend lines.
fn draw_combo_chart(
    canvas: &Canvas,
    px: f64,
    py: f64,
    pw: f64,
    ph: f64,
    primary: &ChartData,
    secondary: &ChartData,
    _trendline: Trendline,
) {
    // Left Y axis (primary).
    let primary_max = primary
        .series
        .iter()
        .flat_map(|(_, v)| v.iter())
        .fold(0.0_f64, |a, b| a.max(*b))
        .max(1e-9);
    let ymax_l = nice_ceil(primary_max);
    let ymin_l = 0.0_f64;
    let span_l = (ymax_l - ymin_l).max(1e-9);
    // Right Y axis (secondary).
    let secondary_max = secondary
        .series
        .iter()
        .flat_map(|(_, v)| v.iter())
        .fold(0.0_f64, |a, b| a.max(*b))
        .max(1e-9);
    let ymax_r = nice_ceil(secondary_max);
    let ymin_r = 0.0_f64;
    let span_r = (ymax_r - ymin_r).max(1e-9);

    // Plot box: shrink on the right by 22 px for the right Y axis.
    let pxw = (pw - 22.0).max(40.0);

    // Gridlines + left Y labels.
    canvas.set_font("10px Arial");
    canvas.set_text_align("right");
    let ticks = 4;
    for i in 0..=ticks {
        let v = ymin_l + span_l * i as f64 / ticks as f64;
        let gy = py + ph - (v - ymin_l) / span_l * ph;
        canvas.set_stroke_style("#e4e7ea");
        canvas.line(px, gy, px + pxw, gy);
        canvas.set_fill_style("#777777");
        canvas.fill_text(&format_tick(v), px - 5.0, gy + 3.0, None);
    }
    // Right Y labels.
    canvas.set_text_align("left");
    for i in 0..=ticks {
        let v = ymin_r + span_r * i as f64 / ticks as f64;
        let gy = py + ph - (v - ymin_r) / span_r * ph;
        canvas.set_fill_style("#777777");
        canvas.fill_text(&format_tick(v), px + pxw + 5.0, gy + 3.0, None);
    }

    let n = primary.labels.len();
    let group_w = pxw / n as f64;

    // X labels (shared).
    canvas.set_text_align("center");
    for (i, label) in primary.labels.iter().enumerate() {
        let cx = px + group_w * (i as f64 + 0.5);
        canvas.set_fill_style("#777777");
        canvas.fill_text(label, cx, py + ph + 14.0, Some(group_w - 2.0));
    }

    // Primary bars.
    let slot = group_w * 0.8 / primary.series.len() as f64;
    for (si, (_, values)) in primary.series.iter().enumerate() {
        canvas.set_fill_style(PALETTE[si % PALETTE.len()]);
        for (i, v) in values.iter().enumerate() {
            let bx = px + group_w * i as f64 + group_w * 0.1 + slot * si as f64;
            let top = py + ph - (v - ymin_l) / span_l * ph;
            let bottom = py + ph;
            canvas.fill_rect(bx, top, (slot - 2.0).max(1.0), (bottom - top).max(1.0));
        }
    }

    // Secondary line overlay, scaled to the right Y axis.
    for (si, (_, values)) in secondary.series.iter().enumerate() {
        // Continue the palette from where primary left off so the
        // first secondary line is a distinct colour from the first
        // primary bar.
        let color = PALETTE[(si + primary.series.len()) % PALETTE.len()];
        canvas.set_stroke_style(color);
        canvas.set_line_width(2.0);
        canvas.begin_path();
        for (i, v) in values.iter().enumerate() {
            let cx = px + group_w * (i as f64 + 0.5);
            let cy = py + ph - (v - ymin_r) / span_r * ph;
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
            let cy = py + ph - (v - ymin_r) / span_r * ph;
            canvas.fill_rect(cx - 2.0, cy - 2.0, 4.0, 4.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn darken_halves_each_channel() {
        // Halve 0x10 → 0x08, floor to 0x08 + 32 = 0x28.
        assert_eq!(darken("#101010"), "#282828");
        // 0xff / 2 + 32 = 0x7f + 0x20 = 0x9f.
        assert_eq!(darken("#ffffff"), "#9f9f9f");
        // 0x80 / 2 + 32 = 0x40 + 0x20 = 0x60.
        assert_eq!(darken("#808080"), "#606060");
    }

    #[test]
    fn darken_falls_back_to_default_on_invalid_input() {
        assert_eq!(darken("not-a-color"), "#333333");
        assert_eq!(darken(""), "#333333");
        assert_eq!(darken("#abc"), "#333333"); // wrong length
    }

    #[test]
    fn with_alpha_produces_rgba() {
        assert_eq!(with_alpha("#1e88e5", 0.25), "rgba(30,136,229,0.25)");
        assert_eq!(with_alpha("#ffffff", 0.5), "rgba(255,255,255,0.5)");
        assert_eq!(with_alpha("#000000", 1.0), "rgba(0,0,0,1)");
    }

    #[test]
    fn with_alpha_clamps_alpha() {
        assert_eq!(with_alpha("#ffffff", 2.0), "rgba(255,255,255,1)");
        assert_eq!(with_alpha("#ffffff", -0.5), "rgba(255,255,255,0)");
    }

    #[test]
    fn with_alpha_falls_back_on_invalid_hex() {
        assert_eq!(with_alpha("not-a-color", 0.3), "rgba(128,128,128,0.3)");
        assert_eq!(with_alpha("#abc", 0.3), "rgba(128,128,128,0.3)");
    }
}

/// Radar / spider chart (Phase 2.1b). One polygon per series, with
/// the categories evenly spaced around a circle. The polygon's
/// vertices are placed at the angle for each category, with the
/// radius scaled by the normalised value. A filled translucent
/// polygon body plus a solid outline so overlapping series stay
/// readable.
fn draw_radar(
    canvas: &Canvas,
    px: f64,
    py: f64,
    pw: f64,
    ph: f64,
    data: &ChartData,
    _trendline: Trendline,
) {
    let n = data.labels.len();
    if n < 3 {
        // A radar chart needs at least 3 axes to form a polygon.
        canvas.set_font("11px Arial");
        canvas.set_fill_style("#999999");
        canvas.fill_text(
            "Radar needs ≥ 3 categories",
            px + pw / 2.0,
            py + ph / 2.0,
            None,
        );
        return;
    }
    let vmax = data
        .series
        .iter()
        .flat_map(|(_, v)| v.iter())
        .fold(0.0_f64, |a, b| a.max(*b))
        .max(1e-9);
    let cx = px + pw / 2.0;
    let cy = py + ph / 2.0;
    // Leave room for category labels around the edge.
    let r_outer = (pw.min(ph) / 2.0 - 14.0).max(10.0);
    // Concentric polygon gridlines (4 rings) for scale reference.
    canvas.set_stroke_style("#e4e7ea");
    canvas.set_line_width(1.0);
    for ring in 1..=4 {
        let r = r_outer * ring as f64 / 4.0;
        canvas.begin_path();
        for i in 0..n {
            // Start at the top (12 o'clock) and go clockwise.
            let theta =
                -std::f64::consts::FRAC_PI_2 + (i as f64) * 2.0 * std::f64::consts::PI / n as f64;
            let x = cx + r * theta.cos();
            let y = cy + r * theta.sin();
            if i == 0 {
                canvas.move_to(x, y);
            } else {
                canvas.line_to(x, y);
            }
        }
        canvas.close_path();
        canvas.stroke();
    }
    // Spokes.
    for i in 0..n {
        let theta =
            -std::f64::consts::FRAC_PI_2 + (i as f64) * 2.0 * std::f64::consts::PI / n as f64;
        let x = cx + r_outer * theta.cos();
        let y = cy + r_outer * theta.sin();
        canvas.begin_path();
        canvas.move_to(cx, cy);
        canvas.line_to(x, y);
        canvas.stroke();
    }
    // Category labels around the outside.
    canvas.set_font("10px Arial");
    canvas.set_text_align("center");
    canvas.set_fill_style("#555555");
    for (i, label) in data.labels.iter().enumerate() {
        let theta =
            -std::f64::consts::FRAC_PI_2 + (i as f64) * 2.0 * std::f64::consts::PI / n as f64;
        let lr = r_outer + 10.0;
        let lx = cx + lr * theta.cos();
        let ly = cy + lr * theta.sin();
        canvas.fill_text(label, lx, ly + 3.0, Some(60.0));
    }
    // One polygon per series. Filled translucent + opaque outline
    // + small dot at each vertex.
    for (si, (_, values)) in data.series.iter().enumerate() {
        let color = PALETTE[si % PALETTE.len()];
        canvas.set_fill_style(&with_alpha(color, 0.25));
        canvas.begin_path();
        for i in 0..n {
            let v = values.get(i).copied().unwrap_or(0.0);
            let r = r_outer * (v / vmax).clamp(0.0, 1.0);
            let theta =
                -std::f64::consts::FRAC_PI_2 + (i as f64) * 2.0 * std::f64::consts::PI / n as f64;
            let x = cx + r * theta.cos();
            let y = cy + r * theta.sin();
            if i == 0 {
                canvas.move_to(x, y);
            } else {
                canvas.line_to(x, y);
            }
        }
        canvas.close_path();
        canvas.fill(None);
        canvas.set_stroke_style(color);
        canvas.set_line_width(2.0);
        canvas.begin_path();
        for i in 0..n {
            let v = values.get(i).copied().unwrap_or(0.0);
            let r = r_outer * (v / vmax).clamp(0.0, 1.0);
            let theta =
                -std::f64::consts::FRAC_PI_2 + (i as f64) * 2.0 * std::f64::consts::PI / n as f64;
            let x = cx + r * theta.cos();
            let y = cy + r * theta.sin();
            if i == 0 {
                canvas.move_to(x, y);
            } else {
                canvas.line_to(x, y);
            }
        }
        canvas.close_path();
        canvas.stroke();
        // Vertex dots.
        canvas.set_fill_style(color);
        for i in 0..n {
            let v = values.get(i).copied().unwrap_or(0.0);
            let r = r_outer * (v / vmax).clamp(0.0, 1.0);
            let theta =
                -std::f64::consts::FRAC_PI_2 + (i as f64) * 2.0 * std::f64::consts::PI / n as f64;
            let x = cx + r * theta.cos();
            let y = cy + r * theta.sin();
            canvas.fill_rect(x - 2.0, y - 2.0, 4.0, 4.0);
        }
    }
}

/// Pie of the first series, with a label + percentage legend on the right.
fn draw_pie(canvas: &Canvas, px: f64, py: f64, pw: f64, ph: f64, data: &ChartData) {
    draw_pie_or_doughnut(canvas, px, py, pw, ph, data, None);
}

/// Doughnut chart (Phase 2.1c) — pie with a hole. Same legend
/// layout as pie, but each slice is a ring wedge (outer arc
/// clockwise + inner arc counter-clockwise).
fn draw_doughnut(canvas: &Canvas, px: f64, py: f64, pw: f64, ph: f64, data: &ChartData) {
    // Inner radius is 45% of the outer radius — matches the
    // default Excel doughnut proportions.
    draw_pie_or_doughnut(canvas, px, py, pw, ph, data, Some(0.45));
}

/// Shared pie / doughnut body. `inner_radius_ratio == None` is a
/// filled pie; `Some(r)` with `0 <= r < 1` cuts a hole of radius
/// `r * outer_radius` (doughnut). The legend on the right is
/// identical for both.
fn draw_pie_or_doughnut(
    canvas: &Canvas,
    px: f64,
    py: f64,
    pw: f64,
    ph: f64,
    data: &ChartData,
    inner_radius_ratio: Option<f64>,
) {
    let Some((_, values)) = data.series.first() else {
        return;
    };
    let total: f64 = values.iter().map(|v| v.abs()).sum();
    if total <= 0.0 {
        return;
    }
    let legend_w = (pw * 0.38).min(120.0);
    let r = ((pw - legend_w).min(ph) / 2.0 - 4.0).max(8.0);
    let ir = inner_radius_ratio.map(|ratio| (r * ratio).max(1.0));
    let (cx, cy) = (px + (pw - legend_w) / 2.0, py + ph / 2.0);

    let mut angle = -std::f64::consts::FRAC_PI_2;
    for (i, v) in values.iter().enumerate() {
        let sweep = v.abs() / total * std::f64::consts::TAU;
        canvas.set_fill_style(PALETTE[i % PALETTE.len()]);
        canvas.begin_path();
        if let Some(ir) = ir {
            // Ring wedge: outer arc clockwise, then line in to the
            // inner radius at the end angle, then inner arc
            // counter-clockwise back to the start angle, then close.
            let start_x = cx + r * angle.cos();
            let start_y = cy + r * angle.sin();
            canvas.move_to(start_x, start_y);
            canvas.arc(cx, cy, r, angle, angle + sweep, None);
            let end_x = cx + ir * (angle + sweep).cos();
            let end_y = cy + ir * (angle + sweep).sin();
            canvas.line_to(end_x, end_y);
            canvas.arc(cx, cy, ir, angle + sweep, angle, Some(true));
            canvas.close_path();
        } else {
            // Solid pie slice.
            canvas.move_to(cx, cy);
            canvas.arc(cx, cy, r, angle, angle + sweep, None);
            canvas.close_path();
        }
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
