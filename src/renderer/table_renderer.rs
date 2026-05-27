#![allow(dead_code)]

use std::fmt::Display;

use crate::renderer::alphabets::exp2xy;
use crate::renderer::canvas::Canvas;
use crate::renderer::render::{AreaRenderer, render};
use web_sys::HtmlCanvasElement;

use super::alphabets::string_at;
use super::viewport::Viewport;
use crate::core::data_proxy::{DataProxy, Style as CellStyle};
use crate::core::cell_range::CellRange;
use crate::core::cell::Cell as DataCell;

/// A snapshot of cells held for copy/cut/paste.
#[derive(Clone)]
pub struct ClipboardData {
    pub r0: usize,
    pub c0: usize,
    pub r1: usize,
    pub c1: usize,
    pub cells: Vec<Vec<DataCell>>,
    pub is_cut: bool,
}

/// An in-progress pointer drag on the headers or scrollbars.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum DragKind {
    ColResize(usize),
    RowResize(usize),
    VScroll,
    HScroll,
}

#[derive(PartialEq, Debug, Clone)]
pub enum Align {
    Left,
    Right,
    Center,
}

impl Display for Align {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Align::Left => "left",
            Align::Right => "right",
            Align::Center => "center",
        };
        write!(f, "{}", s)
    }
}

#[derive(PartialEq, Debug, Clone)]
pub enum VerticalAlign {
    Top,
    Middle,
    Bottom,
}

impl Display for VerticalAlign {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            VerticalAlign::Top => "top",
            VerticalAlign::Middle => "middle",
            VerticalAlign::Bottom => "bottom",
        };
        write!(f, "{}", s)
    }
}

#[derive(PartialEq, Debug, Clone)]
pub enum GridlineStyle {
    Solid,
    Dashed,
    Dotted,
}

#[derive(PartialEq, Debug, Clone)]
pub struct Gridline {
    pub width: f64,
    pub color: String,
    pub style: Option<GridlineStyle>,
}

#[derive(PartialEq, Clone, Copy)]
pub enum TextLineType {
    Underline,
    StrikeThrough,
}

#[derive(PartialEq, Debug, Clone, Copy)]
pub enum BorderType {
    All,
    Inside,
    Horizontal,
    Vertical,
    Outside,
    Left,
    Top,
    Right,
    Bottom,
}

#[derive(PartialEq, Debug, Clone, Copy)]
pub enum BorderLineStyle {
    Thin,
    Medium,
    Thick,
    Dashed,
    Dotted,
}

#[derive(PartialEq, Debug, Copy, Clone)]
pub enum Placement {
    All,
    RowHeader,
    ColHeader,
    Body,
}

pub struct BorderLine {
    pub(crate) left: Option<(BorderLineStyle, String)>,
    pub(crate) top: Option<(BorderLineStyle, String)>,
    pub(crate) right: Option<(BorderLineStyle, String)>,
    pub(crate) bottom: Option<(BorderLineStyle, String)>,
}

#[derive(Debug, Clone)]
pub struct Border {
    pub reference: String,
    pub border_type: BorderType,
    pub border_line: BorderLineStyle,
    pub color: String,
}
#[derive(Debug, Clone)]
pub struct Style {
    pub bgcolor: Option<String>,
    pub color: String,
    pub align: Align,
    pub valign: VerticalAlign,
    pub text_wrap: bool,
    pub underline: bool,
    pub strike_through: bool,
    pub bold: bool,
    pub italic: bool,
    pub font_size: usize,
    pub font_family: String,
    pub rotation: Option<f64>,
    pub padding: Option<(f64, f64)>,
}

impl Default for Style {
    fn default() -> Self {
        Style {
            bgcolor: None,
            color: String::from("#000000"),
            align: Align::Left,
            valign: VerticalAlign::Middle,
            text_wrap: false,
            underline: false,
            strike_through: false,
            bold: false,
            italic: false,
            font_size: 10usize,
            font_family: String::from("Arial"),
            rotation: None,
            padding: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Cell {
    pub value: String,
    pub cell_type: String,
    pub style: usize,
    pub formula: String,
}

#[derive(Debug, Clone, Copy)]
pub struct SelectorRect {
    pub ri: usize,
    pub ci: usize,
    pub eri: usize,
    pub eci: usize,
}

impl Default for SelectorRect {
    fn default() -> Self {
        SelectorRect {
            ri: 0,
            ci: 0,
            eri: 0,
            eci: 0,
        }
    }
}

impl Default for Cell {
    fn default() -> Self {
        Cell {
            value: String::new(),
            cell_type: String::from("text"),
            style: 0,
            formula: String::new(),
        }
    }
}
#[derive(Debug, Clone)]
pub struct Row {
    pub height: f64,
    pub hide: bool,
    pub auto_fit: bool,
    pub style: usize,
}

impl Default for Row {
    fn default() -> Self {
        Row {
            height: 20f64,
            hide: false,
            auto_fit: false,
            style: 0usize,
        }
    }
    
}

#[derive(Debug, Clone)]
pub struct Col {
    pub width: f64,
    pub hide: bool,
    pub auto_fit: bool,
    pub style: usize,
}

impl Default for Col {
    fn default() -> Self {
        Col {
            width: 100f64,
            hide: false,
            auto_fit: false,
            style: 0usize,
        }
    }
}

#[derive(Debug, Clone)]pub struct RowHeader {
    pub width: f64,
    pub cols: usize,
    pub merges: Vec<String>,
}

impl AreaRenderer for RowHeader {
    fn cell(&self, row_index: usize, _: usize) -> Option<Cell> {
        return Some(Cell {
            value: format!("{}", row_index + 1),
            cell_type: String::from("text"),
            style: 0usize,
            formula: String::from(""),
        })
    }

    fn get_merges(&self) -> Vec<String> {
        self.merges.clone()
    }

    fn cell_render(&self, canvas: &Canvas, rect: &Rect, cell: &Cell, style: &str) -> bool {
        return true;
    }
}

#[derive(Debug, Clone)]
pub struct ColHeader {
    pub height: f64,
    pub rows: usize,
    pub merges: Vec<String>,
}

impl AreaRenderer for ColHeader {
    fn cell(&self, _: usize, col_index: usize) -> Option<Cell> {
        return Some(Cell {
            value: string_at(col_index),
            cell_type: String::from("text"),
            style: 0usize,
            formula: String::from(""),
        })
    }

    fn get_merges(&self) -> Vec<String> {
        self.merges.clone()
    }

    fn cell_render(&self, canvas: &Canvas, rect: &Rect, cell: &Cell, style: &str) -> bool {
        canvas.set_fill_style("#0069c2")
            .begin_path()
            .move_to(rect.width - 12f64, 2f64)
            .line_to(rect.width - 2f64, 2f64)
            .line_to(rect.width - 7f64, 10f64)
            .close_path()
            .fill(None);
        return true;
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
#[derive(Debug, Clone)]
pub struct AreaCell {
    pub row: usize,
    pub col: usize,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
#[derive(Debug, Clone)]
pub struct ViewportCell {
    pub placement: Placement,
    pub row: usize,
    pub col: usize,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

pub struct TableRenderer {
    pub target: HtmlCanvasElement,
    pub data: DataProxy,
    pub bgcolor: String,
    pub width: f64,
    pub height: f64,
    pub scale: f64,
    pub rows: usize,
    pub cols: usize,
    pub row_height: f64,
    pub col_width: f64,
    pub start_row: usize,
    pub start_col: usize,
    pub scroll_rows: usize,
    pub scroll_cols: usize,
    pub cell_renderer: Box<dyn Fn(Canvas, Rect, Cell, String) -> bool + 'static>,
    pub formatter: Box<dyn Fn(Cell, String) + 'static>,
    pub merges: Vec<&'static str>,
    pub borders: Vec<Border>,
    pub styles: Vec<Style>,
    pub gridline: Gridline,
    pub style: Style, // default style
    pub col_header: ColHeader,
    pub row_header: RowHeader,
    pub header_gridline: Gridline,
    pub header_style: Style,
    pub freeze: (usize, usize),
    pub freeze_gridline: Gridline,
    pub viewport: Option<Viewport>,
    pub selector: SelectorRect,
    /// Fixed corner of a drag/shift selection (the original mousedown cell).
    pub selection_anchor: (usize, usize),
    pub clipboard: Option<ClipboardData>,
    /// Undo/redo snapshots of the active sheet's data.
    undo_stack: Vec<DataProxy>,
    redo_stack: Vec<DataProxy>,
}

impl TableRenderer {
    pub fn new(container: HtmlCanvasElement, width: f64, height: f64, data: DataProxy) -> TableRenderer {
        TableRenderer {
            target: container,
            data,
            bgcolor: String::from("#ffffff"),
            width,
            height,
            scale: 1f64,
            rows: 5,
            cols: 5,
            row_height: 20f64,
            col_width: 100f64,
            start_row: 0usize,
            start_col: 0usize,
            scroll_rows: 0,
            scroll_cols: 0,
            cell_renderer: Box::new(|_, _, _, _| true),
            formatter: Box::new(|_, _| {}),
            merges: vec![],
            borders: vec![],
            styles: vec![],
            gridline: Gridline {
                width: 1f64,
                color: String::from("#e6e6e6"),
                style: None,
            },
            style: Style::default(),
            col_header: ColHeader {
                height: 20f64,
                rows: 1usize,
                merges: vec![],
            },
            row_header: RowHeader {
                width: 50f64,
                cols: 1usize,
                merges: vec![],
            },
            header_gridline: Gridline {
                width: 1f64,
                color: String::from("#e6e6e6"),
                style: None,
            },
            header_style: Style {
                bgcolor: None,
                color: String::from("#000000"),
                align: Align::Left,
                valign: VerticalAlign::Middle,
                text_wrap: false,
                underline: false,
                strike_through: false,
                bold: false,
                italic: false,
                font_size: 10usize,
                font_family: String::from("Arial"),
                rotation: None,
                padding: None,
            },
            freeze: (0usize, 0usize),
            freeze_gridline: Gridline {
                width: 1f64,
                color: String::from("#e6e6e6"),
                style: None,
            },
            viewport: None,
            selector: SelectorRect::default(),
            selection_anchor: (0, 0),
            clipboard: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }

    pub fn render(&mut self) {
        self.viewport = Some(Viewport::new(
            self.data.clone(),
            self.freeze,
            self.start_row,
            self.start_col,
            self.data.row_count(),
            self.data.col_count(),
            self.width, self.height,
            self.scroll_rows, self.scroll_cols,
            self.row_header.clone(),
            self.col_header.clone()));

        render(self);
    }

    fn bgcolor(&mut self, color: String) -> &Self {
        self.bgcolor = color;
        return self;
    }

    fn width(&mut self, width: f64) -> &Self {
        self.width = width;
        return self;
    }

    fn height(&mut self, height: f64) -> &Self {
        self.height = height;
        return self;
    }

    pub fn scale(&mut self, scale: f64) -> &Self {
        self.scale = scale;
        return self;
    }

    fn rows(&mut self, rows: usize) -> &Self {
        self.rows = rows;
        return self;
    }

    fn cols(&mut self, cols: usize) -> &Self {
        self.cols = cols;
        return self;
    }

    fn row_height(&mut self, row_height: f64) -> &Self {
        self.row_height = row_height;
        return self;
    }

    fn col_width(&mut self, col_width: f64) -> &Self {
        self.col_width = col_width;
        return self;
    }

    fn start_row(&mut self, start_row: usize) -> &Self {
        self.start_row = start_row;
        return self;
    }

    fn start_col(&mut self, start_col: usize) -> &Self {
        self.start_col = start_col;
        return self;
    }

    fn scroll_rows(&mut self, scroll_rows: usize) -> &Self {
        self.scroll_rows = scroll_rows;
        return self;
    }

    fn scroll_cols(&mut self, scroll_cols: usize) -> &Self {
        self.scroll_cols = scroll_cols;
        return self;
    }

    fn cell_renderer<F>(&mut self, cell_renderer: F) -> &Self 
    where F: Fn(Canvas, Rect, Cell, String) -> bool + 'static
    {
        self.cell_renderer = Box::new(cell_renderer);
        return self;
    }

    fn formatter<F>(&mut self, formatter: F) -> &Self 
    where F: Fn(Cell, String) + 'static
    {
        self.formatter = Box::new(formatter);
        return self;
    }

    fn merges(&mut self, merges: Vec<&'static str>) -> &Self {
        self.merges = merges;
        return self;
    }

    fn styles(&mut self, styles: Vec<Style>) -> &Self {
        self.styles = styles;
        return self;
    }

    fn borders(&mut self, borders: Vec<Border>) -> &Self {
        self.borders = borders;
        return self;
    }

    fn gridline(&mut self, gridline: Gridline) -> &Self {
        self.gridline = gridline;
        return self;
    }

    fn style(&mut self, style: Style) -> &Self {
        self.style = style;
        return self;
    }

    fn row_header(&mut self, row_header: RowHeader) -> &Self {
        self.row_header = row_header;
        return self;
    }

    fn col_header(&mut self, col_header: ColHeader) -> &Self {
        self.col_header = col_header;
        return self;
    }

    fn header_gridline(&mut self, header_gridline: Gridline) -> &Self {
        self.header_gridline = header_gridline;
        return self;
    }

    fn header_style(&mut self, header_style: Style) -> &Self {
        self.header_style = header_style;
        return self;
    }

    pub fn freeze(&mut self, reference: &str) -> &Self {
        let (x, y) = exp2xy(reference);
        self.freeze = (y, x);
        return self;
    }

    fn freeze_gridline(&mut self, freeze_gridline: Gridline) -> &Self {
        self.freeze_gridline = freeze_gridline;
        return self;
    }

    pub fn row_height_at(&self, index: usize) -> f64 {
        self.data.get_row_height(index)
    }

    pub fn col_width_at(&self, index: usize) -> f64 {
        self.data.get_col_width(index)
    }

    pub fn set_selector(&mut self, ri: usize, ci: usize, eri: usize, eci: usize) {
        self.selector = SelectorRect { ri, ci, eri, eci };
    }

    /// First body column/row currently scrolled into view.
    fn body_start_col(&self) -> usize {
        self.freeze.1 + self.scroll_cols
    }

    fn body_start_row(&self) -> usize {
        self.freeze.0 + self.scroll_rows
    }

    /// Map a canvas pixel position to a (row, col) cell index. Returns None for
    /// the header gutters. Handles the scrolled body (no-freeze) case.
    pub fn cell_at(&self, x: f64, y: f64) -> Option<(usize, usize)> {
        let tx = self.row_header.width;
        let ty = self.col_header.height;
        if x < tx || y < ty {
            return None;
        }

        let total_cols = self.data.col_count();
        let total_rows = self.data.row_count();
        if total_cols == 0 || total_rows == 0 {
            return None;
        }

        let mut cx = tx;
        let mut ci = self.body_start_col();
        while ci < total_cols {
            let w = self.col_width_at(ci);
            if x < cx + w {
                break;
            }
            cx += w;
            ci += 1;
        }
        if ci >= total_cols {
            ci = total_cols - 1;
        }

        let mut cy = ty;
        let mut ri = self.body_start_row();
        while ri < total_rows {
            let h = self.row_height_at(ri);
            if y < cy + h {
                break;
            }
            cy += h;
            ri += 1;
        }
        if ri >= total_rows {
            ri = total_rows - 1;
        }

        Some((ri, ci))
    }

    /// On-screen rect (canvas pixels) of a cell currently in the body viewport.
    pub fn cell_screen_rect(&self, ri: usize, ci: usize) -> Rect {
        let mut x = self.row_header.width;
        for c in self.body_start_col()..ci {
            x += self.col_width_at(c);
        }
        let mut y = self.col_header.height;
        for r in self.body_start_row()..ri {
            y += self.row_height_at(r);
        }
        Rect {
            x,
            y,
            width: self.col_width_at(ci),
            height: self.row_height_at(ri),
        }
    }

    /// Scroll the body by whole-cell steps, clamped to the data bounds.
    pub fn scroll_by(&mut self, d_rows: i32, d_cols: i32) {
        let max_row = self.data.row_count().saturating_sub(1);
        let max_col = self.data.col_count().saturating_sub(1);
        let nr = (self.scroll_rows as i32 + d_rows).clamp(0, max_row as i32);
        let nc = (self.scroll_cols as i32 + d_cols).clamp(0, max_col as i32);
        self.scroll_rows = nr as usize;
        self.scroll_cols = nc as usize;
    }

    /// Move the single-cell selection by a delta, clamped to the data bounds.
    /// Scrolls the body if the new selection falls outside the visible range.
    pub fn move_selection(&mut self, d_rows: i32, d_cols: i32) {
        let max_row = self.data.row_count().saturating_sub(1) as i32;
        let max_col = self.data.col_count().saturating_sub(1) as i32;
        // Navigate from the selection's edge in the direction of travel so we
        // step past merged regions instead of staying inside them.
        let (r0, c0, r1, c1) = self.selection_bounds();
        let base_r = if d_rows > 0 { r1 } else { r0 };
        let base_c = if d_cols > 0 { c1 } else { c0 };
        let nr = (base_r as i32 + d_rows).clamp(0, max_row) as usize;
        let nc = (base_c as i32 + d_cols).clamp(0, max_col) as usize;
        self.select_cell(nr, nc);
        self.ensure_visible(nr, nc);
    }

    /// Select a cell. If it falls inside a merge, the whole merged range is
    /// selected (with the merge origin as the active cell).
    pub fn select_cell(&mut self, ri: usize, ci: usize) {
        self.selection_anchor = (ri, ci);
        let (r0, c0, r1, c1) = self.data.expand_range_with_merges(ri, ci, ri, ci);
        self.selector = SelectorRect { ri: r0, ci: c0, eri: r1, eci: c1 };
    }

    /// Select a cell and scroll it into view.
    pub fn select_and_reveal(&mut self, ri: usize, ci: usize) {
        self.select_cell(ri, ci);
        self.ensure_visible(ri, ci);
    }

    /// Cells whose raw text contains `query` (case-insensitive), in row-major
    /// order. Empty query yields no matches.
    pub fn find_matches(&self, query: &str) -> Vec<(usize, usize)> {
        let q = query.to_lowercase();
        if q.is_empty() {
            return Vec::new();
        }
        let mut out = Vec::new();
        for ri in 0..self.data.row_count() {
            for ci in 0..self.data.col_count() {
                let text = self.data.get_cell_text(ri, ci);
                if !text.is_empty() && text.to_lowercase().contains(&q) {
                    out.push((ri, ci));
                }
            }
        }
        out
    }

    /// Replace occurrences of `find` with `replace` in a single cell's text.
    /// Returns true if the cell changed.
    pub fn replace_in_cell(&mut self, ri: usize, ci: usize, find: &str, replace: &str) -> bool {
        if find.is_empty() {
            return false;
        }
        let text = self.data.get_cell_text(ri, ci);
        let new_text = replace_ci(&text, find, replace);
        if new_text != text {
            self.snapshot();
            self.data.set_cell_text(ri, ci, &new_text);
            true
        } else {
            false
        }
    }

    /// Replace across every matching cell in one undo step. Returns the number
    /// of cells changed.
    pub fn replace_all(&mut self, find: &str, replace: &str) -> usize {
        if find.is_empty() {
            return 0;
        }
        let matches = self.find_matches(find);
        if matches.is_empty() {
            return 0;
        }
        self.snapshot();
        let mut n = 0;
        for (ri, ci) in matches {
            let text = self.data.get_cell_text(ri, ci);
            let new_text = replace_ci(&text, find, replace);
            if new_text != text {
                self.data.set_cell_text(ri, ci, &new_text);
                n += 1;
            }
        }
        n
    }

    /// Extend the selection to (ri, ci) for drag/shift-select, growing the
    /// rectangle to fully cover any merges it touches. Anchored on the fixed
    /// mousedown cell so dragging in any direction works.
    pub fn select_to(&mut self, ri: usize, ci: usize) {
        let (ar, ac) = self.selection_anchor;
        let (r0, c0, r1, c1) = self.data.expand_range_with_merges(
            ar.min(ri),
            ac.min(ci),
            ar.max(ri),
            ac.max(ci),
        );
        self.selector = SelectorRect { ri: r0, ci: c0, eri: r1, eci: c1 };
    }

    /// The origin (top-left) of the merge covering (ri, ci), else (ri, ci).
    pub fn merge_origin(&self, ri: usize, ci: usize) -> (usize, usize) {
        match self.data.cell_merge(ri, ci) {
            Some(m) => (m.sri, m.sci),
            None => (ri, ci),
        }
    }

    /// Adjust scroll so (ri, ci) is within the body viewport.
    fn ensure_visible(&mut self, ri: usize, ci: usize) {
        if ri < self.body_start_row() {
            self.scroll_rows = ri.saturating_sub(self.freeze.0);
        }
        if ci < self.body_start_col() {
            self.scroll_cols = ci.saturating_sub(self.freeze.1);
        }
        // Scroll down/right until the target fits in the viewport.
        let avail_h = self.height - self.col_header.height;
        let avail_w = self.width - self.row_header.width;
        while ri > self.body_start_row() {
            let visible: f64 = (self.body_start_row()..=ri).map(|r| self.row_height_at(r)).sum();
            if visible <= avail_h { break; }
            self.scroll_rows += 1;
        }
        while ci > self.body_start_col() {
            let visible: f64 = (self.body_start_col()..=ci).map(|c| self.col_width_at(c)).sum();
            if visible <= avail_w { break; }
            self.scroll_cols += 1;
        }
    }

    pub fn set_cell_text_at(&mut self, ri: usize, ci: usize, text: &str) {
        self.snapshot();
        self.data.set_cell_text(ri, ci, text);
    }

    /// Replace the active sheet data (used when switching sheet tabs), resetting
    /// the view state and undo history.
    pub fn set_data(&mut self, data: DataProxy) {
        self.freeze = data.freeze;
        self.data = data;
        self.start_row = 0;
        self.start_col = 0;
        self.scroll_rows = 0;
        self.scroll_cols = 0;
        self.selector = SelectorRect::default();
        self.undo_stack.clear();
        self.redo_stack.clear();
    }

    /// Record the current data on the undo stack before a mutation, dropping
    /// the redo history. Call at the start of every user-initiated edit.
    fn snapshot(&mut self) {
        const MAX_UNDO: usize = 100;
        self.undo_stack.push(self.data.clone());
        if self.undo_stack.len() > MAX_UNDO {
            self.undo_stack.remove(0);
        }
        self.redo_stack.clear();
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    pub fn undo(&mut self) {
        if let Some(prev) = self.undo_stack.pop() {
            self.redo_stack.push(self.data.clone());
            self.data = prev;
        }
    }

    pub fn redo(&mut self) {
        if let Some(next) = self.redo_stack.pop() {
            self.undo_stack.push(self.data.clone());
            self.data = next;
        }
    }

    pub fn data_clone(&self) -> DataProxy {
        self.data.clone()
    }

    /// Normalized selection bounds (top-left .. bottom-right).
    fn selection_bounds(&self) -> (usize, usize, usize, usize) {
        let s = self.selector;
        (
            s.ri.min(s.eri),
            s.ci.min(s.eci),
            s.ri.max(s.eri),
            s.ci.max(s.eci),
        )
    }

    /// Apply a style mutation to every cell in the current selection.
    pub fn update_selection_style<F: Fn(&mut CellStyle)>(&mut self, f: F) {
        self.snapshot();
        let (r0, c0, r1, c1) = self.selection_bounds();
        for ri in r0..=r1 {
            for ci in c0..=c1 {
                let mut style = self.data.get_cell_style(ri, ci);
                f(&mut style);
                let idx = self.data.add_style(style);
                self.data.set_cell_style(ri, ci, idx);
            }
        }
    }

    pub fn toggle_bold(&mut self) {
        let t = !self.data.get_cell_style(self.selector.ri, self.selector.ci).bold;
        self.update_selection_style(move |s| s.bold = t);
    }

    pub fn toggle_italic(&mut self) {
        let t = !self.data.get_cell_style(self.selector.ri, self.selector.ci).italic;
        self.update_selection_style(move |s| s.italic = t);
    }

    pub fn toggle_underline(&mut self) {
        let t = !self.data.get_cell_style(self.selector.ri, self.selector.ci).underline;
        self.update_selection_style(move |s| s.underline = t);
    }

    pub fn toggle_strike(&mut self) {
        let t = !self.data.get_cell_style(self.selector.ri, self.selector.ci).strike;
        self.update_selection_style(move |s| s.strike = t);
    }

    pub fn set_align(&mut self, align: &str) {
        let a = align.to_string();
        self.update_selection_style(move |s| s.align = a.clone());
    }

    pub fn set_valign(&mut self, valign: &str) {
        let a = valign.to_string();
        self.update_selection_style(move |s| s.valign = a.clone());
    }

    pub fn set_bgcolor(&mut self, color: &str) {
        let c = color.to_string();
        self.update_selection_style(move |s| s.bgcolor = Some(c.clone()));
    }

    pub fn set_text_color(&mut self, color: &str) {
        let c = color.to_string();
        self.update_selection_style(move |s| s.color = c.clone());
    }

    pub fn toggle_text_wrap(&mut self) {
        let t = !self.data.get_cell_style(self.selector.ri, self.selector.ci).text_wrap;
        self.update_selection_style(move |s| s.text_wrap = t);
    }

    pub fn set_format(&mut self, format: &str) {
        let f = format.to_string();
        self.update_selection_style(move |s| s.format = f.clone());
    }

    pub fn set_font_family(&mut self, family: &str) {
        let f = family.to_string();
        self.update_selection_style(move |s| s.font_family = f.clone());
    }

    pub fn set_font_size(&mut self, px: usize) {
        self.update_selection_style(move |s| s.font_size = px);
    }

    /// Remove styling from every cell in the selection.
    pub fn clear_format(&mut self) {
        self.snapshot();
        let (r0, c0, r1, c1) = self.selection_bounds();
        for ri in r0..=r1 {
            for ci in c0..=c1 {
                if let Some(cell) = self.data.get_cell_mut(ri, ci) {
                    cell.style = None;
                }
            }
        }
    }

    /// Merge the selection (or unmerge when a single merged cell is selected).
    pub fn merge_selection(&mut self) {
        self.snapshot();
        let (r0, c0, r1, c1) = self.selection_bounds();
        if r0 == r1 && c0 == c1 {
            if let Some(m) = self.data.cell_merge(r0, c0) {
                self.data.merges.delete(m.sri, m.sci);
            }
            return;
        }
        self.data.merges.add(CellRange::new(r0, c0, r1, c1));
    }

    fn snapshot_clipboard(&mut self, is_cut: bool) {
        let (r0, c0, r1, c1) = self.selection_bounds();
        let mut cells = Vec::new();
        for ri in r0..=r1 {
            let mut row = Vec::new();
            for ci in c0..=c1 {
                row.push(self.data.get_cell(ri, ci).cloned().unwrap_or_default());
            }
            cells.push(row);
        }
        self.clipboard = Some(ClipboardData { r0, c0, r1, c1, cells, is_cut });
    }

    pub fn copy_selection(&mut self) {
        self.snapshot_clipboard(false);
    }

    pub fn cut_selection(&mut self) {
        self.snapshot_clipboard(true);
    }

    /// Paste the clipboard at the current selection's top-left. A cut clears
    /// the source cells afterwards.
    pub fn paste(&mut self) {
        if self.clipboard.is_none() {
            return;
        }
        self.snapshot();
        let Some(cb) = self.clipboard.take() else { return };
        let (dr0, dc0, _, _) = self.selection_bounds();
        for (i, row) in cb.cells.iter().enumerate() {
            for (j, cell) in row.iter().enumerate() {
                self.data.set_cell(dr0 + i, dc0 + j, cell.clone());
            }
        }
        if cb.is_cut {
            for ri in cb.r0..=cb.r1 {
                for ci in cb.c0..=cb.c1 {
                    self.data.delete_cell(ri, ci);
                }
            }
        } else {
            self.clipboard = Some(cb); // keep for repeated paste
        }
    }

    pub fn clear_selection_content(&mut self) {
        self.snapshot();
        let (r0, c0, r1, c1) = self.selection_bounds();
        for ri in r0..=r1 {
            for ci in c0..=c1 {
                self.data.delete_cell(ri, ci);
            }
        }
    }

    pub fn insert_row_at_selection(&mut self) {
        self.snapshot();
        let (r0, _, _, _) = self.selection_bounds();
        self.data.insert_row(r0, 1);
    }

    pub fn delete_rows_at_selection(&mut self) {
        self.snapshot();
        let (r0, _, r1, _) = self.selection_bounds();
        for _ in r0..=r1 {
            self.data.delete_row(r0);
        }
    }

    pub fn insert_col_at_selection(&mut self) {
        self.snapshot();
        let (_, c0, _, _) = self.selection_bounds();
        self.data.insert_col(c0, 1);
    }

    pub fn delete_cols_at_selection(&mut self) {
        self.snapshot();
        let (_, c0, _, c1) = self.selection_bounds();
        for _ in c0..=c1 {
            self.data.delete_col(c0);
        }
    }

    /// Freeze panes at the selection's top-left, or unfreeze if already there.
    pub fn toggle_freeze(&mut self) {
        let (r0, c0, _, _) = self.selection_bounds();
        if self.freeze == (r0, c0) {
            self.freeze = (0, 0);
        } else {
            self.freeze = (r0, c0);
        }
    }

    pub fn cell_text_at(&self, ri: usize, ci: usize) -> String {
        self.data.get_cell_text(ri, ci)
    }

    pub fn get_selector(&self) -> SelectorRect {
        self.selector
    }

    /// Set the canvas cursor (e.g. "col-resize", "row-resize", "default").
    pub fn set_cursor(&self, cursor: &str) {
        let _ = self.target.style().set_property("cursor", cursor);
    }

    /// If the pointer is near a header boundary, the column/row it would resize.
    pub fn resize_target(&self, x: f64, y: f64) -> Option<DragKind> {
        let tx = self.row_header.width;
        let ty = self.col_header.height;
        let tol = 4f64;

        // Column boundary, within the column-header strip.
        if y <= ty && x >= tx {
            let mut cx = tx;
            let mut ci = self.body_start_col();
            let total = self.data.col_count();
            while ci < total && cx <= self.width {
                let right = cx + self.col_width_at(ci);
                if (x - right).abs() <= tol {
                    return Some(DragKind::ColResize(ci));
                }
                cx = right;
                ci += 1;
            }
        }

        // Row boundary, within the row-header gutter.
        if x <= tx && y >= ty {
            let mut cy = ty;
            let mut ri = self.body_start_row();
            let total = self.data.row_count();
            while ri < total && cy <= self.height {
                let bottom = cy + self.row_height_at(ri);
                if (y - bottom).abs() <= tol {
                    return Some(DragKind::RowResize(ri));
                }
                cy = bottom;
                ri += 1;
            }
        }

        None
    }

    /// If the pointer is over a scrollbar track, which one.
    pub fn scrollbar_target(&self, x: f64, y: f64) -> Option<DragKind> {
        let size = 12f64;
        if x >= self.width - size && y >= self.col_header.height && y <= self.height {
            return Some(DragKind::VScroll);
        }
        if y >= self.height - size && x >= self.row_header.width && x <= self.width {
            return Some(DragKind::HScroll);
        }
        None
    }

    pub fn set_col_width_clamped(&mut self, ci: usize, w: f64) {
        self.data.set_col_width(ci, w.max(20f64));
    }

    pub fn set_row_height_clamped(&mut self, ri: usize, h: f64) {
        self.data.set_row_height(ri, h.max(12f64));
    }

    /// Scroll vertically to a fraction [0, 1] of the rows.
    pub fn scroll_to_fraction_v(&mut self, frac: f64) {
        let total = self.data.row_count();
        let idx = (frac.clamp(0f64, 1f64) * total as f64) as usize;
        self.scroll_rows = idx.min(total.saturating_sub(1));
    }

    /// Scroll horizontally to a fraction [0, 1] of the columns.
    pub fn scroll_to_fraction_h(&mut self, frac: f64) {
        let total = self.data.col_count();
        let idx = (frac.clamp(0f64, 1f64) * total as f64) as usize;
        self.scroll_cols = idx.min(total.saturating_sub(1));
    }

    pub fn set_row_height_at(&mut self, row_index: usize, height: f64) {
        self.data.set_row_height(row_index, height);
    }

    pub fn set_col_width_at(&mut self, col_index: usize, width: f64) {
        self.data.set_col_width(col_index, width);
    }
}

/// Lowercase a single char (first mapping; fine for find/replace use).
fn lc(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

/// Case-insensitive replace of all (non-overlapping) occurrences of `find`.
fn replace_ci(haystack: &str, find: &str, replace: &str) -> String {
    let hs: Vec<char> = haystack.chars().collect();
    let fs: Vec<char> = find.chars().collect();
    let flen = fs.len();
    if flen == 0 {
        return haystack.to_string();
    }
    let mut out = String::new();
    let mut i = 0usize;
    while i < hs.len() {
        if i + flen <= hs.len() && (0..flen).all(|k| lc(hs[i + k]) == lc(fs[k])) {
            out.push_str(replace);
            i += flen;
        } else {
            out.push(hs[i]);
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::replace_ci;

    #[test]
    fn case_insensitive_replace() {
        assert_eq!(replace_ci("Hello hello HELLO", "hello", "hi"), "hi hi hi");
        assert_eq!(replace_ci("abcabc", "bc", "X"), "aXaX");
        assert_eq!(replace_ci("nothing", "zzz", "Y"), "nothing");
        assert_eq!(replace_ci("keep", "", "x"), "keep");
    }
}
