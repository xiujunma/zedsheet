#![allow(dead_code)]

use std::fmt::Display;

use crate::renderer::alphabets::exp2xy;
use crate::renderer::canvas::Canvas;
use crate::renderer::multi_range::MultiRangeState;
use crate::renderer::render::{render, AreaRenderer};
use web_sys::HtmlCanvasElement;

use super::alphabets::string_at;
use super::viewport::Viewport;
use crate::core::auto_filter::Sort;
use crate::core::cell::Cell as DataCell;
use crate::core::cell_range::CellRange;
use crate::core::clipboard_io::ParsedGrid;
use crate::core::data_proxy::{
    ActiveSheet, DataProxy, PageBreak, SheetsRegistry, Style as CellStyle,
};
use crate::core::sparkline::{Sparkline, SparklineKind};

/// A snapshot of cells held for copy/cut/paste.
#[derive(Clone)]
pub struct ClipboardData {
    pub r0: usize,
    pub c0: usize,
    pub r1: usize,
    pub c1: usize,
    pub cells: Vec<Vec<DataCell>>,
    /// Computed values captured at copy time (row-major, parallel to `cells`),
    /// so "Paste Values" can drop formulas that wouldn't re-evaluate detached
    /// from their source sheet (issue #28).
    pub values: Vec<Vec<String>>,
    pub is_cut: bool,
}

/// What a paste applies to the destination (issue #28). `All` is the ordinary
/// full-fidelity paste; the rest are Paste Special variants.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PasteMode {
    All,
    Values,
    Formulas,
    Formats,
    Transpose,
    Link,
}

/// The single write a paste makes to one destination cell, decided purely from
/// the source cell and mode so it can be unit-tested without a renderer.
#[derive(Clone, Debug)]
pub enum CellWrite {
    /// Replace the whole cell (full fidelity).
    Full(DataCell),
    /// Set only the cell's text/formula, keeping the destination's formatting.
    Text(String),
    /// Apply only a style index, keeping the destination's content.
    Style(usize),
    /// Leave the destination cell untouched.
    Skip,
}

/// Decide what a paste writes to one cell. `value` is the source's captured
/// computed value (for `Values`); `src_ref` is the source cell's A1 reference
/// (for `Link`).
fn paste_cell_plan(mode: PasteMode, cell: &DataCell, value: &str, src_ref: &str) -> CellWrite {
    match mode {
        PasteMode::All | PasteMode::Transpose => CellWrite::Full(cell.clone()),
        PasteMode::Values => CellWrite::Text(value.to_string()),
        PasteMode::Formulas => CellWrite::Text(cell.text.clone()),
        PasteMode::Formats => match cell.style {
            Some(idx) => CellWrite::Style(idx),
            None => CellWrite::Skip,
        },
        PasteMode::Link => CellWrite::Text(format!("={src_ref}")),
    }
}

/// What the Merge toolbar button should do for the current selection.
#[derive(Debug, PartialEq, Eq)]
enum MergeAction {
    Merge,
    Unmerge,
    NoOp,
}

/// Decide the Merge button's action. `merge_at_origin` is the bounds
/// `(sri, sci, eri, eci)` of an existing merge anchored at the selection's
/// top-left, if any. Pure so it can be unit-tested.
///
/// Selecting a merged cell auto-expands the selection to cover the whole merge
/// (see `select_cell`), so "the selection exactly equals an existing merge" is
/// the signal to toggle it off. Without this, unmerge is unreachable from the
/// UI: a single-cell check never matches because the click already expanded
/// the selection to the full (multi-cell) merge range.
fn merge_toggle_action(
    sel: (usize, usize, usize, usize),
    merge_at_origin: Option<(usize, usize, usize, usize)>,
) -> MergeAction {
    if merge_at_origin == Some(sel) {
        MergeAction::Unmerge
    } else if sel.0 == sel.2 && sel.1 == sel.3 {
        MergeAction::NoOp
    } else {
        MergeAction::Merge
    }
}

/// Transpose a clipboard block (swap rows/columns), including each cell's merge
/// span `(extra_rows, extra_cols)`. Pure so it can be unit-tested.
fn transpose_clipboard(cb: &ClipboardData) -> ClipboardData {
    let rows = cb.cells.len();
    let cols = cb.cells.first().map_or(0, Vec::len);
    let mut cells: Vec<Vec<DataCell>> = (0..cols).map(|_| Vec::with_capacity(rows)).collect();
    let mut values: Vec<Vec<String>> = (0..cols).map(|_| Vec::with_capacity(rows)).collect();
    for i in 0..rows {
        for j in 0..cols {
            let mut cell = cb.cells[i][j].clone();
            if let Some((rs, cs)) = cell.merge {
                cell.merge = Some((cs, rs));
            }
            cells[j].push(cell);
            values[j].push(
                cb.values
                    .get(i)
                    .and_then(|r| r.get(j))
                    .cloned()
                    .unwrap_or_default(),
            );
        }
    }
    ClipboardData {
        r0: cb.r0,
        c0: cb.c0,
        r1: cb.r0 + cols.saturating_sub(1),
        c1: cb.c0 + rows.saturating_sub(1),
        cells,
        values,
        is_cut: false,
    }
}

/// An in-progress pointer drag on the headers or scrollbars.
#[derive(Clone, PartialEq, Debug)]
pub enum DragKind {
    ColResize(usize),
    RowResize(usize),
    VScroll,
    HScroll,
    /// Dragging the selection's bottom-right fill handle.
    Fill,
    /// Moving a slicer panel (Phase 1.1, issue #61 follow-on).
    /// Carries the slicer's stable id; the drag-start panel geometry
    /// lives on `DragState.start_panel_*` so mousemove doesn't have to
    /// re-read the data model on every tick.
    SlicerDrag(String),
    /// Resizing a slicer panel via its bottom-right grip.
    SlicerResize(String),
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
    /// Two parallel lines instead of one. Excel calls this "double".
    Double,
}

#[derive(PartialEq, Debug, Copy, Clone)]
pub enum Placement {
    All,
    RowHeader,
    ColHeader,
    Body,
}

#[derive(Default)]
pub struct BorderLine {
    pub(crate) left: Option<(BorderLineStyle, String)>,
    pub(crate) top: Option<(BorderLineStyle, String)>,
    pub(crate) right: Option<(BorderLineStyle, String)>,
    pub(crate) bottom: Option<(BorderLineStyle, String)>,
    pub(crate) diagonal_up: Option<(BorderLineStyle, String)>,
    pub(crate) diagonal_down: Option<(BorderLineStyle, String)>,
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

/// Base header sizes; the outline gutters (issue #30) extend them.
pub const BASE_ROW_HEADER_W: f64 = 50.0;
pub const BASE_COL_HEADER_H: f64 = 20.0;
/// Width of one outline nesting-level lane in the gutter.
pub const OUTLINE_LANE: f64 = 12.0;

/// A click target in the outline gutters (issue #30).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OutlineHit {
    RowToggle(usize),
    ColToggle(usize),
    RowLevel(usize),
    ColLevel(usize),
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SelectorRect {
    pub ri: usize,
    pub ci: usize,
    pub eri: usize,
    pub eci: usize,
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

#[derive(Debug, Clone)]
pub struct RowHeader {
    pub width: f64,
    pub cols: usize,
    pub merges: Vec<String>,
}

impl AreaRenderer for RowHeader {
    fn cell(&self, row_index: usize, _: usize) -> Option<Cell> {
        Some(Cell {
            value: format!("{}", row_index + 1),
            cell_type: String::from("text"),
            style: 0usize,
            formula: String::from(""),
        })
    }

    fn get_merges(&self) -> Vec<String> {
        self.merges.clone()
    }

    fn cell_render(&self, _canvas: &Canvas, _rect: &Rect, _cell: &Cell, _style: &str) -> bool {
        true
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
        Some(Cell {
            value: string_at(col_index),
            cell_type: String::from("text"),
            style: 0usize,
            formula: String::from(""),
        })
    }

    fn get_merges(&self) -> Vec<String> {
        self.merges.clone()
    }

    fn cell_render(&self, canvas: &Canvas, rect: &Rect, _cell: &Cell, _style: &str) -> bool {
        canvas
            .set_fill_style("#0069c2")
            .begin_path()
            .move_to(rect.width - 12f64, 2f64)
            .line_to(rect.width - 2f64, 2f64)
            .line_to(rect.width - 7f64, 10f64)
            .close_path()
            .fill(None);
        true
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
    /// Ctrl/Cmd multi-range selection (issue #19). Empty = single-rect mode.
    /// The last range's anchor is the active cell mirrored by `selector`.
    pub multi_range: MultiRangeState,
    pub clipboard: Option<ClipboardData>,
    /// Format Painter (issue #31): the source cell's resolved style, captured
    /// while armed; the next selection applies it. `None` when not armed.
    format_painter: Option<CellStyle>,
    /// Source selection bounds (r0,c0,r1,c1) while a fill-handle drag is active.
    fill_source: Option<(usize, usize, usize, usize)>,
    /// Screen rect (x,y,w,h) of the fill handle from the last render, for hit-testing.
    last_fill_handle: std::cell::Cell<Option<(f64, f64, f64, f64)>>,
    /// Undo/redo snapshots of the **workbook** (issue #62). Each entry
    /// captures the full sheets registry + the active index, so any
    /// cross-sheet mutator (pivot, add sheet, etc.) is fully reversible.
    /// For mutators that only touch `self.data` (cell edits, styling) the
    /// snapshot is a length-1 registry — still cheap because `DataProxy`
    /// is mostly text cells and styles.
    undo_stack: Vec<WorkbookSnapshot>,
    redo_stack: Vec<WorkbookSnapshot>,
    // Workbook-level undo (issue #62): the old bespoke `pivot_undo`
    // field is gone — the unified `undo_stack` (of `WorkbookSnapshot`)
    // captures the same information and every cross-sheet mutator
    // pushes through it.
}

/// A point-in-time snapshot of the workbook state (issue #62). Cloning
/// the full `Vec<DataProxy>` is the cost of correct cross-sheet undo —
/// the previous per-sheet `Vec<DataProxy>` snapshot couldn't capture
/// "the user added a new sheet" or "a pivot pushed a sibling sheet",
/// so it silently lost work. The new shape is the same as
/// `PivotUndo` from issue #35, generalized.
#[derive(Clone)]
pub(crate) struct WorkbookSnapshot {
    pub(crate) active: usize,
    pub(crate) sheets: Vec<DataProxy>,
}

impl TableRenderer {
    pub fn new(
        container: HtmlCanvasElement,
        width: f64,
        height: f64,
        data: DataProxy,
    ) -> TableRenderer {
        let mut r = TableRenderer {
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
                // Light-gray fill so the row/column headers read as a distinct
                // gutter, not part of the data area (x-spreadsheet uses this).
                bgcolor: Some(String::from("#f4f5f8")),
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
            // The frozen-pane divider is a heavier, darker line than the normal
            // gridlines (which are 1px #e6e6e6) so the freeze boundary reads as a
            // distinct edge — matching Excel's frozen-pane border.
            freeze_gridline: Gridline {
                width: 1f64,
                color: String::from("#a6a6a6"),
                style: None,
            },
            viewport: None,
            selector: SelectorRect::default(),
            selection_anchor: (0, 0),
            multi_range: MultiRangeState::new(),
            clipboard: None,
            format_painter: None,
            fill_source: None,
            last_fill_handle: std::cell::Cell::new(None),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        };
        // Data may arrive with outline groups (load_data / tests) — size the
        // gutters before the first render (issue #30).
        r.refresh_outline_gutters();
        r
    }

    pub fn render(&mut self) {
        self.viewport = Some(Viewport::new(
            self.data.clone(),
            self.freeze,
            self.start_row,
            self.start_col,
            self.data.row_count(),
            self.data.col_count(),
            self.width,
            self.height,
            self.scroll_rows,
            self.scroll_cols,
            self.row_header.clone(),
            self.col_header.clone(),
        ));

        render(self);
    }

    fn bgcolor(&mut self, color: String) -> &Self {
        self.bgcolor = color;
        self
    }

    fn width(&mut self, width: f64) -> &Self {
        self.width = width;
        self
    }

    fn height(&mut self, height: f64) -> &Self {
        self.height = height;
        self
    }

    pub fn scale(&mut self, scale: f64) -> &Self {
        self.scale = scale;
        self
    }

    fn rows(&mut self, rows: usize) -> &Self {
        self.rows = rows;
        self
    }

    fn cols(&mut self, cols: usize) -> &Self {
        self.cols = cols;
        self
    }

    fn row_height(&mut self, row_height: f64) -> &Self {
        self.row_height = row_height;
        self
    }

    fn col_width(&mut self, col_width: f64) -> &Self {
        self.col_width = col_width;
        self
    }

    fn start_row(&mut self, start_row: usize) -> &Self {
        self.start_row = start_row;
        self
    }

    fn start_col(&mut self, start_col: usize) -> &Self {
        self.start_col = start_col;
        self
    }

    fn scroll_rows(&mut self, scroll_rows: usize) -> &Self {
        self.scroll_rows = scroll_rows;
        self
    }

    fn scroll_cols(&mut self, scroll_cols: usize) -> &Self {
        self.scroll_cols = scroll_cols;
        self
    }

    fn cell_renderer<F>(&mut self, cell_renderer: F) -> &Self
    where
        F: Fn(Canvas, Rect, Cell, String) -> bool + 'static,
    {
        self.cell_renderer = Box::new(cell_renderer);
        self
    }

    fn formatter<F>(&mut self, formatter: F) -> &Self
    where
        F: Fn(Cell, String) + 'static,
    {
        self.formatter = Box::new(formatter);
        self
    }

    fn merges(&mut self, merges: Vec<&'static str>) -> &Self {
        self.merges = merges;
        self
    }

    fn styles(&mut self, styles: Vec<Style>) -> &Self {
        self.styles = styles;
        self
    }

    fn borders(&mut self, borders: Vec<Border>) -> &Self {
        self.borders = borders;
        self
    }

    fn gridline(&mut self, gridline: Gridline) -> &Self {
        self.gridline = gridline;
        self
    }

    fn style(&mut self, style: Style) -> &Self {
        self.style = style;
        self
    }

    fn row_header(&mut self, row_header: RowHeader) -> &Self {
        self.row_header = row_header;
        self
    }

    fn col_header(&mut self, col_header: ColHeader) -> &Self {
        self.col_header = col_header;
        self
    }

    fn header_gridline(&mut self, header_gridline: Gridline) -> &Self {
        self.header_gridline = header_gridline;
        self
    }

    fn header_style(&mut self, header_style: Style) -> &Self {
        self.header_style = header_style;
        self
    }

    pub fn freeze(&mut self, reference: &str) -> &Self {
        let (x, y) = exp2xy(reference);
        self.freeze = (y, x);
        self
    }

    fn freeze_gridline(&mut self, freeze_gridline: Gridline) -> &Self {
        self.freeze_gridline = freeze_gridline;
        self
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
    pub(crate) fn body_start_col(&self) -> usize {
        self.freeze.1 + self.scroll_cols
    }

    pub(crate) fn body_start_row(&self) -> usize {
        self.freeze.0 + self.scroll_rows
    }

    /// Map a canvas pixel position to a (row, col) cell index. Returns None for
    /// the header gutters. Frozen panes are handled: clicks land on the pinned
    /// rows/columns or the scrolled body depending on where the frozen band
    /// ends (see [`track_at`]).
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

        let ci = track_at(
            x,
            tx,
            self.start_col,
            self.freeze.1,
            self.body_start_col(),
            total_cols,
            |c| self.col_width_at(c),
        );
        let ri = track_at(
            y,
            ty,
            self.start_row,
            self.freeze.0,
            self.body_start_row(),
            total_rows,
            |r| self.row_height_at(r),
        );

        Some((ri, ci))
    }

    /// On-screen rect (canvas pixels) of a cell currently in view. Frozen-pane
    /// aware so the drawn rect (and the editor positioned over it) lines up with
    /// where [`cell_at`] hit-tests the same cell.
    pub fn cell_screen_rect(&self, ri: usize, ci: usize) -> Rect {
        let x = track_offset(
            ci,
            self.row_header.width,
            self.start_col,
            self.freeze.1,
            self.body_start_col(),
            |c| self.col_width_at(c),
        );
        let y = track_offset(
            ri,
            self.col_header.height,
            self.start_row,
            self.freeze.0,
            self.body_start_row(),
            |r| self.row_height_at(r),
        );
        Rect {
            x,
            y,
            width: self.col_width_at(ci),
            height: self.row_height_at(ri),
        }
    }

    /// The on-screen rect of a cell, expanded to cover its full merged region
    /// when it is part of one (the top-left comes from the merge origin). Used
    /// to size the cell editor so it covers the whole merged cell, not just the
    /// anchor.
    pub fn merged_screen_rect(&self, ri: usize, ci: usize) -> Rect {
        let (r0, c0, r1, c1) = self.data.expand_range_with_merges(ri, ci, ri, ci);
        let origin = self.cell_screen_rect(r0, c0);
        Rect {
            x: origin.x,
            y: origin.y,
            width: (c0..=c1).map(|c| self.col_width_at(c)).sum(),
            height: (r0..=r1).map(|r| self.row_height_at(r)).sum(),
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

    // --- Row/column outline groups (issue #30) ---

    /// Resize the header bands to make room for the outline gutters: one
    /// 12px lane per nesting level. Call whenever the group lists may have
    /// changed (group/ungroup, data swap, undo). Headers are unzoomed by
    /// design (issue #32), and so is the gutter.
    pub fn refresh_outline_gutters(&mut self) {
        let row_lvls = crate::core::outline::max_level(&self.data.row_groups);
        let col_lvls = crate::core::outline::max_level(&self.data.col_groups);
        self.row_header.width = BASE_ROW_HEADER_W
            + if row_lvls > 0 {
                row_lvls as f64 * OUTLINE_LANE + 4.0
            } else {
                0.0
            };
        self.col_header.height = BASE_COL_HEADER_H
            + if col_lvls > 0 {
                col_lvls as f64 * OUTLINE_LANE + 4.0
            } else {
                0.0
            };
    }

    /// Group the selection's rows (context menu). Document state → undoable.
    pub fn group_rows(&mut self) {
        if self.data.is_read_only() {
            return;
        }
        self.snapshot();
        let (r0, _, r1, _) = self.selection_bounds();
        self.data.add_row_group(r0, r1);
        self.refresh_outline_gutters();
    }

    pub fn ungroup_rows(&mut self) {
        if self.data.is_read_only() {
            return;
        }
        self.snapshot();
        let (r0, _, r1, _) = self.selection_bounds();
        self.data.remove_row_groups_overlapping(r0, r1);
        self.refresh_outline_gutters();
    }

    pub fn group_cols(&mut self) {
        if self.data.is_read_only() {
            return;
        }
        self.snapshot();
        let (_, c0, _, c1) = self.selection_bounds();
        self.data.add_col_group(c0, c1);
        self.refresh_outline_gutters();
    }

    pub fn ungroup_cols(&mut self) {
        if self.data.is_read_only() {
            return;
        }
        self.snapshot();
        let (_, c0, _, c1) = self.selection_bounds();
        self.data.remove_col_groups_overlapping(c0, c1);
        self.refresh_outline_gutters();
    }

    /// Insert SUBTOTAL rows for the selection, grouping each key block
    /// (issue #30 — Excel's Data ▸ Subtotal).
    pub fn subtotal_selection(&mut self) {
        if self.data.is_read_only() {
            return;
        }
        self.snapshot();
        let (r0, c0, r1, c1) = self.selection_bounds();
        self.data.subtotal_range(r0, c0, r1, c1);
        self.refresh_outline_gutters();
    }

    /// "Format as Table" over the current selection (issue #34).
    /// Phase 4.5: convert every cell in the current selection
    /// to a single-run rich-text cell. The run inherits each
    /// cell's existing style index so the visual stays the same
    /// after the conversion; the only change is the cell now
    /// renders via the runs path. A no-op for an empty
    /// selection or a read-only sheet.
    pub fn format_selection_as_rich(&mut self) {
        if self.data.is_read_only() {
            return;
        }
        self.snapshot();
        let range = self.data.selector.range.clone();
        for r in range.sri..=range.eri {
            for c in range.sci..=range.eci {
                if let Some(cell) = self.data.get_cell_mut(r, c) {
                    let style = cell.style;
                    cell.convert_to_rich(style);
                }
            }
        }
    }

    /// Phase 4.5: revert every cell in the current selection back
    /// to the flat \`text\` / \`style\` rendering by dropping
    /// \`runs\`. The cell text + style index are preserved (the run
    /// text was a clone of \`text\`), so the visual is identical.
    pub fn convert_selection_to_plain(&mut self) {
        if self.data.is_read_only() {
            return;
        }
        self.snapshot();
        let range = self.data.selector.range.clone();
        for r in range.sri..=range.eri {
            for c in range.sci..=range.eci {
                if let Some(cell) = self.data.get_cell_mut(r, c) {
                    cell.runs = None;
                }
            }
        }
    }

    pub fn format_selection_as_table(&mut self) {
        if self.data.is_read_only() {
            return;
        }
        self.snapshot();
        let range = self.data.selector.range.clone();
        self.data.format_as_table(&range);
    }

    /// Toggle the totals row of the table under the active cell (issue #34).
    pub fn toggle_table_totals_at_selection(&mut self) {
        if self.data.is_read_only() {
            return;
        }
        let (ri, ci) = (self.data.selector.ri, self.data.selector.ci);
        let Some(name) = self.data.table_at(ri, ci).map(|t| t.name.clone()) else {
            return;
        };
        self.snapshot();
        self.data.toggle_table_totals(&name);
    }

    /// Convert the table under the active cell back to a plain range
    /// (issue #34).
    pub fn convert_table_at_selection(&mut self) {
        if self.data.is_read_only() {
            return;
        }
        let (ri, ci) = (self.data.selector.ri, self.data.selector.ci);
        let Some(name) = self.data.table_at(ri, ci).map(|t| t.name.clone()) else {
            return;
        };
        self.snapshot();
        self.data.convert_table_to_range(&name);
    }

    /// Collapse/expand toggles and level buttons. Not snapshotted — Excel
    /// does not put collapse/expand on the undo stack either.
    pub fn toggle_outline(&mut self, hit: OutlineHit) {
        match hit {
            OutlineHit::RowToggle(i) => self.data.toggle_row_group(i),
            OutlineHit::ColToggle(i) => self.data.toggle_col_group(i),
            OutlineHit::RowLevel(k) => self.data.set_row_outline_level(k),
            OutlineHit::ColLevel(k) => self.data.set_col_outline_level(k),
        }
    }

    /// Screen rect of each row group's ± toggle (group index, rect). The
    /// toggle sits in its level's gutter lane at the summary row (end + 1),
    /// which stays visible when the group collapses.
    pub fn row_group_toggle_rects(&self) -> Vec<(usize, Rect)> {
        let levels = crate::core::outline::group_levels(&self.data.row_groups);
        let max_row = self.data.row_count().saturating_sub(1);
        self.data
            .row_groups
            .iter()
            .enumerate()
            .filter_map(|(i, g)| {
                let lane = (levels[i] - 1) as f64 * OUTLINE_LANE;
                let toggle_row = (g.end + 1).min(max_row);
                let y = self.cell_screen_rect(toggle_row, 0).y;
                if y < self.col_header.height - 0.5 || y >= self.height {
                    return None; // scrolled out of view
                }
                Some((
                    i,
                    Rect {
                        x: lane + 1.0,
                        y: y + 1.0,
                        width: 11.0,
                        height: 11.0,
                    },
                ))
            })
            .collect()
    }

    pub fn col_group_toggle_rects(&self) -> Vec<(usize, Rect)> {
        let levels = crate::core::outline::group_levels(&self.data.col_groups);
        let max_col = self.data.col_count().saturating_sub(1);
        self.data
            .col_groups
            .iter()
            .enumerate()
            .filter_map(|(i, g)| {
                let lane = (levels[i] - 1) as f64 * OUTLINE_LANE;
                let toggle_col = (g.end + 1).min(max_col);
                let x = self.cell_screen_rect(0, toggle_col).x;
                if x < self.row_header.width - 0.5 || x >= self.width {
                    return None;
                }
                Some((
                    i,
                    Rect {
                        x: x + 1.0,
                        y: lane + 1.0,
                        width: 11.0,
                        height: 11.0,
                    },
                ))
            })
            .collect()
    }

    /// Rects of the 1/2/…/n outline level buttons in the corner: row buttons
    /// along the top strip, column buttons in a second strip below.
    pub fn row_level_button_rects(&self) -> Vec<(usize, Rect)> {
        let max = crate::core::outline::max_level(&self.data.row_groups);
        if max == 0 {
            return Vec::new();
        }
        (1..=max + 1)
            .map(|k| {
                (
                    k,
                    Rect {
                        x: 2.0 + (k - 1) as f64 * 13.0,
                        y: 2.0,
                        width: 11.0,
                        height: 11.0,
                    },
                )
            })
            .collect()
    }

    pub fn col_level_button_rects(&self) -> Vec<(usize, Rect)> {
        let max = crate::core::outline::max_level(&self.data.col_groups);
        if max == 0 {
            return Vec::new();
        }
        (1..=max + 1)
            .map(|k| {
                (
                    k,
                    Rect {
                        x: 2.0 + (k - 1) as f64 * 13.0,
                        y: 15.0,
                        width: 11.0,
                        height: 11.0,
                    },
                )
            })
            .collect()
    }

    /// Hit-test the outline gutters: toggles and level buttons. Checked
    /// before resize/cell handling in the mousedown path.
    pub fn outline_hit(&self, x: f64, y: f64) -> Option<OutlineHit> {
        let inside = |r: &Rect| x >= r.x && x <= r.x + r.width && y >= r.y && y <= r.y + r.height;
        for (k, r) in self.row_level_button_rects() {
            if inside(&r) {
                return Some(OutlineHit::RowLevel(k));
            }
        }
        for (k, r) in self.col_level_button_rects() {
            if inside(&r) {
                return Some(OutlineHit::ColLevel(k));
            }
        }
        for (i, r) in self.row_group_toggle_rects() {
            if inside(&r) {
                return Some(OutlineHit::RowToggle(i));
            }
        }
        for (i, r) in self.col_group_toggle_rects() {
            if inside(&r) {
                return Some(OutlineHit::ColToggle(i));
            }
        }
        None
    }

    /// Number of body rows that fit in the viewport — the step for PageUp /
    /// PageDown (issue #41). Approximated from the default row height; always
    /// at least 1 so a page move never stalls.
    pub fn rows_per_page(&self) -> usize {
        let body_h = (self.height - self.col_header.height).max(0.0);
        // Zoomed row height (issue #32): fewer rows fit per page at 200%.
        let rh = (self.data.default_row_height * self.data.zoom()).max(1.0);
        ((body_h / rh).floor() as usize).max(1)
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
    ///
    /// This is the canonical "start a fresh single selection" primitive, so it
    /// also drops any non-contiguous (Ctrl+click) ranges — otherwise keyboard
    /// navigation, editing, find/replace and name-box jumps would leave stale
    /// disjoint ranges active and fan subsequent edits out to them (issue #19).
    pub fn select_cell(&mut self, ri: usize, ci: usize) {
        self.multi_range.clear();
        self.selection_anchor = (ri, ci);
        let (r0, c0, r1, c1) = self.data.expand_range_with_merges(ri, ci, ri, ci);
        self.selector = SelectorRect {
            ri: r0,
            ci: c0,
            eri: r1,
            eci: c1,
        };
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
        let (r0, c0, r1, c1) =
            self.data
                .expand_range_with_merges(ar.min(ri), ac.min(ci), ar.max(ri), ac.max(ci));
        self.selector = SelectorRect {
            ri: r0,
            ci: c0,
            eri: r1,
            eci: c1,
        };
    }

    // --- Multi-range selection (issue #19) ---

    /// All selected ranges, normalized to `(r0, c0, r1, c1)` tuples. Empty
    /// when single-rect mode; the consumer should then fall back to
    /// `selection_bounds()`. This is the iteration source for every fan-out
    /// mutator.
    pub fn selection_ranges(&self) -> Vec<(usize, usize, usize, usize)> {
        if self.multi_range.is_active() {
            self.multi_range.normalized()
        } else {
            let (r0, c0, r1, c1) = self.selection_bounds();
            vec![(r0, c0, r1, c1)]
        }
    }

    /// Bounding box of all selected ranges; falls back to `selection_bounds()`
    /// in single-rect mode. Used by `set_borders` "outer" mode.
    pub fn union_bounds(&self) -> (usize, usize, usize, usize) {
        if self.multi_range.is_active() {
            self.multi_range
                .union()
                .unwrap_or_else(|| self.selection_bounds())
        } else {
            self.selection_bounds()
        }
    }

    /// True if `(ri, ci)` lies inside any selected range (or inside the single
    /// `selector` rect in single-rect mode).
    pub fn contains_selected(&self, ri: usize, ci: usize) -> bool {
        if self.multi_range.is_active() {
            self.multi_range.contains(ri, ci)
        } else {
            let (r0, c0, r1, c1) = self.selection_bounds();
            ri >= r0 && ri <= r1 && ci >= c0 && ci <= c1
        }
    }

    /// Walk every cell in every selected range. In single-rect mode this
    /// visits every cell of `selection_bounds()`.
    pub fn for_each_selected_cell<F: FnMut(usize, usize)>(&self, mut f: F) {
        if self.multi_range.is_active() {
            self.multi_range.for_each_cell(f);
        } else {
            let (r0, c0, r1, c1) = self.selection_bounds();
            for ri in r0..=r1 {
                for ci in c0..=c1 {
                    f(ri, ci);
                }
            }
        }
    }

    /// Ctrl/Cmd-click equivalent: push a new range (anchor = the click cell,
    /// size = 1×1 for now). Bounds are merge-expanded so a click inside a
    /// merged region doesn't shrink to a single cell. Mirror the new anchor
    /// into `selector` so the formula bar / name box reflect the active cell.
    pub fn add_range(&mut self, r0: usize, c0: usize, r1: usize, c1: usize) {
        let (r0, c0, r1, c1) = self.data.expand_range_with_merges(r0, c0, r1, c1);
        self.multi_range.add(r0, c0, r1, c1);
        self.selector = SelectorRect {
            ri: r0,
            ci: c0,
            eri: r1,
            eci: c1,
        };
        self.selection_anchor = (r0, c0);
    }

    /// Reset the multi-range. `selection_anchor` returns to the active rect's
    /// top-left so a subsequent plain drag extends from the right place.
    pub fn clear_multi_range(&mut self) {
        self.multi_range.clear();
        self.selection_anchor = (self.selector.ri, self.selector.ci);
    }

    /// Whether the multi-range has at least one Ctrl/Cmd-added entry.
    pub fn multi_range_is_active(&self) -> bool {
        self.multi_range.is_active()
    }

    /// Promote the current single-rect `selector` into the multi-range. Used
    /// by the first Ctrl/Cmd-click so the user's prior click isn't lost.
    /// `selection_anchor` is set to the selector's top-left so a subsequent
    /// Ctrl+drag extends the new range from there.
    pub fn promote_selector_to_range(&mut self) {
        let s = self.selector;
        self.multi_range.add(s.ri, s.ci, s.eri, s.eci);
        self.selection_anchor = (s.ri, s.ci);
    }

    /// Ctrl/Cmd-drag equivalent: grow the most-recently-added range from
    /// `selection_anchor` to `(ri, ci)`, mirror the new top-left into
    /// `selector`, and update the anchor. No-op in single-rect mode.
    pub fn select_to_last(&mut self, ri: usize, ci: usize) {
        if !self.multi_range.is_active() {
            self.select_to(ri, ci);
            return;
        }
        let (ar, ac) = self.selection_anchor;
        let (r0, c0, r1, c1) =
            self.data
                .expand_range_with_merges(ar.min(ri), ac.min(ci), ar.max(ri), ac.max(ci));
        // Pass the raw cursor: extend_last normalizes against its own anchor, so
        // this grows the range in every direction. Passing the pre-maxed
        // (r1, c1) collapsed reverse (up/left) drags to the anchor (issue #19).
        self.multi_range.extend_last(ri, ci);
        self.selector = SelectorRect {
            ri: r0,
            ci: c0,
            eri: r1,
            eci: c1,
        };
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
            let visible: f64 = (self.body_start_row()..=ri)
                .map(|r| self.row_height_at(r))
                .sum();
            if visible <= avail_h {
                break;
            }
            self.scroll_rows += 1;
        }
        while ci > self.body_start_col() {
            let visible: f64 = (self.body_start_col()..=ci)
                .map(|c| self.col_width_at(c))
                .sum();
            if visible <= avail_w {
                break;
            }
            self.scroll_cols += 1;
        }
    }

    /// Set the text of a single cell, honoring both read-only / per-cell
    /// locking (issue #24) and any data-validation rule on the target
    /// cell (issue #9).
    ///
    /// Returns `Ok(())` on success or when the cell is locked (locked
    /// cells are silently a no-op, matching the prior behavior). Returns
    /// `Err(message)` when validation fails — the cell is **not** written
    /// and the undo stack is **not** pushed, so a rejected commit leaves
    /// no junk entries behind.
    pub fn set_cell_text_at(&mut self, ri: usize, ci: usize, text: &str) -> Result<(), String> {
        // Honor read-only mode and per-cell locking (issue #24). The data
        // layer also gates this as a safety net, but checking here lets us
        // avoid recording a no-op on the undo stack.
        if !self.data.is_cell_editable(ri, ci) {
            return Ok(());
        }
        // Validate first (issue #9). On failure, surface the message and
        // skip both the snapshot and the write so undo/redo stay clean.
        if !self.data.validations.validate(ri, ci, text) {
            if let Some(msg) = self.data.validations.get_error(ri, ci) {
                return Err(msg.clone());
            }
            return Err("Invalid value".to_string());
        }
        self.snapshot();
        self.data.set_cell_text(ri, ci, text);
        // Typing adjacent to a table grows it (issue #34).
        self.data.maybe_expand_tables(ri, ci);
        Ok(())
    }

    /// Attach a validator to every cell in `ref_str` (e.g. `"A1:B3"`).
    /// Snapshots before mutating so the change is undoable. Caller is
    /// responsible for `render()`.
    pub fn set_validations_for_range(
        &mut self,
        ref_str: &str,
        validator: crate::core::validation::Validator,
    ) {
        self.snapshot();
        self.data.validations.add("cell", ref_str, validator);
    }

    /// Remove any validator that covers any cell in `ref_str`. Snapshots
    /// before mutating.
    pub fn clear_validations_in_range(&mut self, ref_str: &str) {
        if let Ok(cr) = crate::core::cell_range::CellRange::from_str(ref_str) {
            self.snapshot();
            self.data.validations.remove(&cr);
        }
    }

    /// True iff the cell has a list-type validator (drives the ▼ glyph
    /// and the list popover).
    pub fn cell_has_list_validator(&self, ri: usize, ci: usize) -> bool {
        self.data
            .validations
            .get(ri, ci)
            .map(|v| v.validator.type_ == "list")
            .unwrap_or(false)
    }

    /// The allowed values for a list-validator cell, or `None` if the
    /// cell has no list validator. Values are trimmed; empty entries
    /// are dropped.
    pub fn list_values_for_cell(&self, ri: usize, ci: usize) -> Option<Vec<String>> {
        let v = self.data.validations.get(ri, ci)?;
        if v.validator.type_ != "list" {
            return None;
        }
        Some(
            v.validator
                .value
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
        )
    }

    /// Replace the active sheet data (used when switching sheet tabs), resetting
    /// the view state and undo history.
    pub fn set_data(&mut self, data: DataProxy) {
        // Zoom is per-view, not per-document: keep the current level across a
        // data swap (load_data / sheet-tab switch), since the incoming
        // DataProxy carries the transient default of 1.0 (issue #32).
        let zoom = self.data.zoom();
        self.freeze = data.freeze;
        self.data = data;
        self.start_row = 0;
        self.start_col = 0;
        self.scroll_rows = 0;
        self.scroll_cols = 0;
        self.data.set_zoom(zoom);
        // Size the outline gutters for the loaded data's groups (issue #30).
        self.refresh_outline_gutters();
        // Restore the selection carried in the loaded data instead of snapping
        // back to A1 (issue #44). For payloads without `sel`, DataProxy's
        // selector defaults to A1, so this preserves the old behavior.
        let sel = self.data.selector.clone();
        self.selector = SelectorRect {
            ri: sel.range.sri,
            ci: sel.range.sci,
            eri: sel.range.eri,
            eci: sel.range.eci,
        };
        self.selection_anchor = (sel.ri, sel.ci);
        self.multi_range.clear();
        // Note (issue #62): set_data no longer clears the undo/redo
        // stacks. The previous behavior was right for the per-sheet
        // `load_data` path, but for cross-sheet mutators (`add_pivot`,
        // `refresh_active_pivot`) the snapshot is pushed BEFORE this call
        // and must survive it. Callers that load fresh data (the JS
        // `load_data` API) clear the stacks explicitly.
    }

    /// Record the current data on the undo stack before a mutation, dropping
    /// the redo history. Call at the start of every user-initiated edit.
    /// Captures a workbook snapshot (issue #62): the active sheet + its
    /// index. Cross-sheet mutators (add_pivot, add_sheet, etc.) call
    /// [`snapshot_workbook`] instead, which clones the full registry.
    pub(crate) fn snapshot(&mut self) {
        const MAX_UNDO: usize = 100;
        self.undo_stack.push(WorkbookSnapshot {
            active: 0, // Overwritten by callers that know the real index;
            // for a pure-data mutator the active index didn't
            // change so 0 is the right "no-op" for the registry
            // restore path.
            sheets: vec![self.data.clone()],
        });
        if self.undo_stack.len() > MAX_UNDO {
            self.undo_stack.remove(0);
        }
        self.redo_stack.clear();
    }

    /// Workbook-level snapshot (issue #62): call from any mutator that
    /// touches `sheets` or the active index — add_pivot, add_sheet,
    /// remove_sheet, switch_sheet, etc. Captures the full registry so
    /// the undo restores every sheet and the active selection.
    pub(crate) fn snapshot_workbook(&mut self, sheets: &SheetsRegistry, active: &ActiveSheet) {
        const MAX_UNDO: usize = 100;
        self.undo_stack.push(WorkbookSnapshot {
            active: *active.borrow(),
            sheets: sheets.borrow().clone(),
        });
        if self.undo_stack.len() > MAX_UNDO {
            self.undo_stack.remove(0);
        }
        self.redo_stack.clear();
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    /// Drop both undo and redo history. Called by the JS `load_data`
    /// API right before swapping in a freshly-loaded workbook (issue #62) —
    /// the user's prior edits and pivots are no longer relevant to the
    /// new data.
    pub fn clear_undo(&mut self) {
        self.undo_stack.clear();
        self.redo_stack.clear();
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    pub fn undo(&mut self, sheets: &SheetsRegistry, active: &ActiveSheet) {
        if let Some(prev) = self.undo_stack.pop() {
            // The renderer doesn't own the registry; snapshot the current
            // workbook state for redo before we touch anything.
            let zoom = self.data.zoom();
            let current_active = *active.borrow();
            let current_sheets = sheets.borrow().clone();
            self.redo_stack.push(WorkbookSnapshot {
                active: current_active,
                sheets: current_sheets,
            });
            // Restore the registry + active index, then swap self.data
            // to the restored active sheet (issue #62).
            *sheets.borrow_mut() = prev.sheets.clone();
            *active.borrow_mut() = prev.active;
            self.data = prev.sheets[prev.active].clone();
            self.data.set_zoom(zoom);
            self.refresh_outline_gutters();
        }
    }

    pub fn redo(&mut self, sheets: &SheetsRegistry, active: &ActiveSheet) {
        if let Some(next) = self.redo_stack.pop() {
            let zoom = self.data.zoom();
            let current_active = *active.borrow();
            let current_sheets = sheets.borrow().clone();
            self.undo_stack.push(WorkbookSnapshot {
                active: current_active,
                sheets: current_sheets,
            });
            *sheets.borrow_mut() = next.sheets.clone();
            *active.borrow_mut() = next.active;
            self.data = next.sheets[next.active].clone();
            self.data.set_zoom(zoom);
            self.refresh_outline_gutters();
        }
    }

    /// Current view zoom factor (1.0 = 100%).
    pub fn zoom(&self) -> f64 {
        self.data.zoom()
    }

    /// Set the view zoom (clamped to 10–400%). View-only: no undo snapshot.
    pub fn set_zoom(&mut self, zoom: f64) {
        self.data.set_zoom(zoom);
    }

    /// Insert a manual row page break (Phase 5.1) after `row`. The
    /// break ends the printed page after the given row, so cells in
    /// `row + 1` and below start on the next page. Idempotent:
    /// calling twice on the same row is a no-op.
    pub fn insert_row_page_break(&mut self, row: usize) {
        if self.data.is_read_only() {
            return;
        }
        self.snapshot();
        let pb = PageBreak {
            row: Some(row),
            col: None,
        };
        if !self.data.page_setup.page_breaks.contains(&pb) {
            self.data.page_setup.page_breaks.push(pb);
            self.data
                .page_setup
                .page_breaks
                .sort_by_key(|b| b.row.unwrap_or(0));
        }
    }

    /// Insert a manual column page break after `col`. Idempotent.
    pub fn insert_col_page_break(&mut self, col: usize) {
        if self.data.is_read_only() {
            return;
        }
        self.snapshot();
        let pb = PageBreak {
            row: None,
            col: Some(col),
        };
        if !self.data.page_setup.page_breaks.contains(&pb) {
            self.data.page_setup.page_breaks.push(pb);
            self.data
                .page_setup
                .page_breaks
                .sort_by_key(|b| b.col.unwrap_or(0));
        }
    }

    /// Remove any page break at `row` or `col`. A single break is
    /// either-or (row vs col), so this covers both with one call
    /// from the UI. No-op when nothing matches.
    pub fn remove_page_break(&mut self, row: usize, col: usize) {
        if self.data.is_read_only() {
            return;
        }
        let before = self.data.page_setup.page_breaks.len();
        self.data.page_setup.page_breaks.retain(|b| {
            b.row.map(|r| r != row).unwrap_or(true) && b.col.map(|c| c != col).unwrap_or(true)
        });
        if self.data.page_setup.page_breaks.len() != before {
            self.snapshot();
        }
    }

    /// Add a sparkline (Phase 4.1b/5.2). The new entry is appended
    /// to `DataProxy.sparklines`; rendering reads from there on the
    /// next frame. Snapshots for undo (issue #62). No-op on a
    /// read-only sheet or when `range` / `anchor` is empty.
    pub fn add_sparkline(
        &mut self,
        kind: SparklineKind,
        range: String,
        anchor: String,
        color: String,
    ) {
        if self.data.is_read_only() || range.is_empty() || anchor.is_empty() {
            return;
        }
        self.snapshot();
        self.data.sparklines.push(Sparkline {
            kind,
            range,
            anchor,
            color,
            width: 120.0,
            height: 20.0,
        });
    }

    /// Remove the sparkline whose anchor matches `anchor`. Snapshots
    /// for undo. No-op when no entry matches or on a read-only
    /// sheet.
    pub fn remove_sparkline(&mut self, anchor: &str) {
        if self.data.is_read_only() || anchor.is_empty() {
            return;
        }
        let before = self.data.sparklines.len();
        self.data.sparklines.retain(|s| s.anchor != anchor);
        if self.data.sparklines.len() != before {
            self.snapshot();
        }
    }

    /// Append a shape (Phase 6) to the active sheet. The new
    /// entry is drawn on the next frame. Snapshots for undo
    /// (issue #62). No-op on a read-only sheet or with an empty
    /// anchor.
    pub fn add_shape(&mut self, shape: crate::core::shape::Shape) {
        if self.data.is_read_only() || shape.anchor.is_empty() {
            return;
        }
        self.snapshot();
        self.data.shapes.push(shape);
    }

    /// Remove the shape at `index` (Phase 6). Snapshots for undo
    /// when an entry was actually removed.
    pub fn remove_shape(&mut self, index: usize) {
        if self.data.is_read_only() {
            return;
        }
        if index >= self.data.shapes.len() {
            return;
        }
        self.snapshot();
        self.data.shapes.remove(index);
    }

    pub fn data_clone(&self) -> DataProxy {
        let mut d = self.data.clone();
        // Mirror the live selection into the snapshot so get_data() exposes the
        // active cell + selection range (issue #44). The renderer's
        // SelectorRect is the source of truth; DataProxy.selector is otherwise
        // only a serialization/bridge copy. This also makes tab switches and
        // on_change payloads carry the current selection.
        let s = self.selector;
        d.selector.range = CellRange::new(
            s.ri.min(s.eri),
            s.ci.min(s.eci),
            s.ri.max(s.eri),
            s.ci.max(s.eci),
        );
        d.selector.ri = self.selection_anchor.0;
        d.selector.ci = self.selection_anchor.1;
        d
    }

    /// Normalized selection bounds (top-left .. bottom-right).
    pub(crate) fn selection_bounds(&self) -> (usize, usize, usize, usize) {
        let s = self.selector;
        (
            s.ri.min(s.eri),
            s.ci.min(s.eci),
            s.ri.max(s.eri),
            s.ci.max(s.eci),
        )
    }

    /// Apply a style mutation to every cell in the current selection. With
    /// multi-range selection (issue #19), iterates over every range.
    pub fn update_selection_style<F: Fn(&mut CellStyle)>(&mut self, f: F) {
        // Read-only sheets reject all formatting (issue #24). This is the funnel
        // for every style toggle (bold/align/color/format/font/rotation/…), so
        // guarding it here covers the toolbar, keyboard shortcuts, color palette
        // and dropdowns in one place.
        if self.data.is_read_only() {
            return;
        }
        self.snapshot();
        let cells: Vec<(usize, usize)> = {
            let mut v = Vec::new();
            self.for_each_selected_cell(|ri, ci| v.push((ri, ci)));
            v
        };
        for (ri, ci) in cells {
            // Per-cell format-lock (Phase 4.2 follow-on): a cell
            // can be value-editable but format-locked, so the
            // sheet-wide read-only check is necessary but not
            // sufficient. Honor the per-cell flag here.
            if !self.data.is_cell_format_editable(ri, ci) {
                continue;
            }
            let mut style = self.data.get_cell_style(ri, ci);
            f(&mut style);
            let idx = self.data.add_style(style);
            self.data.set_cell_style(ri, ci, idx);
        }
    }

    /// Format Painter (issue #31): if armed, disarm; otherwise capture the
    /// active cell's resolved style to paint onto the next selection. Returns
    /// the resulting armed state (for cursor feedback).
    pub fn toggle_format_painter(&mut self) -> bool {
        self.format_painter = match self.format_painter {
            Some(_) => None,
            None => Some(self.data.get_cell_style(self.selector.ri, self.selector.ci)),
        };
        self.format_painter.is_some()
    }

    pub fn is_format_painter_armed(&self) -> bool {
        self.format_painter.is_some()
    }

    /// Apply the armed Format Painter style to the current selection, then
    /// disarm. No-op if not armed (or, via the style funnel, on a read-only
    /// sheet). Single-use, matching Excel's single-click Format Painter.
    pub fn apply_format_painter(&mut self) {
        let Some(style) = self.format_painter.take() else {
            return;
        };
        self.update_selection_style(move |s| *s = style.clone());
    }

    pub fn toggle_bold(&mut self) {
        let t = !self
            .data
            .get_cell_style(self.selector.ri, self.selector.ci)
            .bold;
        self.update_selection_style(move |s| s.bold = t);
    }

    pub fn toggle_italic(&mut self) {
        let t = !self
            .data
            .get_cell_style(self.selector.ri, self.selector.ci)
            .italic;
        self.update_selection_style(move |s| s.italic = t);
    }

    pub fn toggle_underline(&mut self) {
        let t = !self
            .data
            .get_cell_style(self.selector.ri, self.selector.ci)
            .underline;
        self.update_selection_style(move |s| s.underline = t);
    }

    pub fn toggle_strike(&mut self) {
        let t = !self
            .data
            .get_cell_style(self.selector.ri, self.selector.ci)
            .strike;
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
        let t = !self
            .data
            .get_cell_style(self.selector.ri, self.selector.ci)
            .text_wrap;
        self.update_selection_style(move |s| s.text_wrap = t);
    }

    /// Set the rotation angle in degrees for the selected cells. `0.0`
    /// (or any `None`-ish value) clears the rotation (issue #25).
    pub fn set_rotation(&mut self, angle: f64) {
        let r = if angle.abs() < 1e-9 {
            None
        } else {
            Some(angle)
        };
        self.update_selection_style(move |s| s.rotation = r);
    }

    /// Toggle shrink-to-fit on the selected cells (issue #25). When on, the
    /// renderer scales the font down so the text fits without wrapping.
    pub fn toggle_shrink_to_fit(&mut self) {
        let t = !self
            .data
            .get_cell_style(self.selector.ri, self.selector.ci)
            .shrink_to_fit;
        self.update_selection_style(move |s| s.shrink_to_fit = t);
    }

    /// Bump the left indent by `delta` pixels (negative `delta` decreases).
    /// Indent is clamped at 0 (issue #25).
    pub fn bump_indent(&mut self, delta: i64) {
        let cur = self
            .data
            .get_cell_style(self.selector.ri, self.selector.ci)
            .indent as i64;
        let next = (cur + delta).max(0) as usize;
        self.update_selection_style(move |s| s.indent = next);
    }

    pub fn set_format(&mut self, format: &str) {
        let f = format.to_string();
        self.update_selection_style(move |s| s.format = f.clone());
    }

    /// Apply borders to the selection. `mode` is one of:
    /// all | outer | none | top | bottom | left | right
    /// | diagonal-up | diagonal-down.
    /// With multi-range selection (issue #19), iterates every range; the
    /// `outer` mode draws each range's OWN perimeter (not the union bbox), so
    /// disjoint ranges each get a complete outer border.
    pub fn set_borders(&mut self, mode: &str) {
        if self.data.is_read_only() {
            return;
        }
        self.snapshot();
        let line = Some(("thin".to_string(), "#000000".to_string()));
        // Per-range so "outer" edges are correct for disjoint selections; a
        // single-rect selection is just one range.
        let ranges = if self.multi_range.is_active() {
            self.multi_range.normalized()
        } else {
            vec![self.selection_bounds()]
        };
        for (r0, c0, r1, c1) in ranges {
            for ri in r0..=r1 {
                for ci in c0..=c1 {
                    let mut style = self.data.get_cell_style(ri, ci);
                    if mode == "none" {
                        style.border = None;
                    } else {
                        let mut b = style.border.clone().unwrap_or_default();
                        let want_top =
                            mode == "all" || mode == "top" || (mode == "outer" && ri == r0);
                        let want_bottom =
                            mode == "all" || mode == "bottom" || (mode == "outer" && ri == r1);
                        let want_left =
                            mode == "all" || mode == "left" || (mode == "outer" && ci == c0);
                        let want_right =
                            mode == "all" || mode == "right" || (mode == "outer" && ci == c1);
                        if want_top {
                            b.top = line.clone();
                        }
                        if want_bottom {
                            b.bottom = line.clone();
                        }
                        if want_left {
                            b.left = line.clone();
                        }
                        if want_right {
                            b.right = line.clone();
                        }
                        if mode == "diagonal-up" {
                            b.diagonal_up = line.clone();
                        }
                        if mode == "diagonal-down" {
                            b.diagonal_down = line.clone();
                        }
                        let empty = b.top.is_none()
                            && b.bottom.is_none()
                            && b.left.is_none()
                            && b.right.is_none()
                            && b.diagonal_up.is_none()
                            && b.diagonal_down.is_none();
                        style.border = if empty { None } else { Some(b) };
                    }
                    let idx = self.data.add_style(style);
                    self.data.set_cell_style(ri, ci, idx);
                }
            }
        }
    }

    pub fn set_font_family(&mut self, family: &str) {
        let f = family.to_string();
        self.update_selection_style(move |s| s.font_family = f.clone());
    }

    pub fn set_font_size(&mut self, px: usize) {
        self.update_selection_style(move |s| s.font_size = px);
    }

    /// Remove styling from every cell in the selection. Fans out across all
    /// ranges in multi-range mode (issue #19).
    pub fn clear_format(&mut self) {
        if self.data.is_read_only() {
            return;
        }
        self.snapshot();
        let cells: Vec<(usize, usize)> = {
            let mut v = Vec::new();
            self.for_each_selected_cell(|ri, ci| v.push((ri, ci)));
            v
        };
        for (ri, ci) in cells {
            if let Some(cell) = self.data.get_cell_mut(ri, ci) {
                cell.style = None;
            }
        }
    }

    /// Merge the selection, or unmerge it when the selection exactly covers an
    /// existing merge (clicking a merged cell expands the selection to the whole
    /// merge, so a second click toggles it off).
    pub fn merge_selection(&mut self) {
        if self.data.is_read_only() {
            return;
        }
        self.snapshot();
        let (r0, c0, r1, c1) = self.selection_bounds();
        let merge_at_origin = self
            .data
            .cell_merge(r0, c0)
            .filter(|m| m.sri == r0 && m.sci == c0)
            .map(|m| (m.sri, m.sci, m.eri, m.eci));
        match merge_toggle_action((r0, c0, r1, c1), merge_at_origin) {
            MergeAction::Unmerge => {
                self.data.merges.delete(r0, c0);
            }
            MergeAction::Merge => {
                self.data.merges.add(CellRange::new(r0, c0, r1, c1));
            }
            MergeAction::NoOp => {}
        }
    }

    /// Capture the selection into the clipboard. Returns `false` (without
    /// touching the clipboard) for a non-contiguous (Ctrl+click) selection,
    /// which can't be represented as one rectangular block (issue #19/H6).
    fn snapshot_clipboard(&mut self, is_cut: bool) -> bool {
        if self.multi_range.is_active() {
            return false;
        }
        let (r0, c0, r1, c1) = self.selection_bounds();
        let mut cells = Vec::new();
        let mut values = Vec::new();
        for ri in r0..=r1 {
            let mut row = Vec::new();
            let mut vrow = Vec::new();
            for ci in c0..=c1 {
                row.push(self.data.get_cell(ri, ci).cloned().unwrap_or_default());
                // Capture the computed value now — a copied formula can't be
                // re-evaluated detached from its source sheet (issue #28).
                vrow.push(self.data.cell_raw_value(ri, ci));
            }
            cells.push(row);
            values.push(vrow);
        }
        self.clipboard = Some(ClipboardData {
            r0,
            c0,
            r1,
            c1,
            cells,
            values,
            is_cut,
        });
        true
    }

    /// Copy the selection; `false` means the selection was non-contiguous and
    /// nothing was copied (the caller should tell the user).
    pub fn copy_selection(&mut self) -> bool {
        self.snapshot_clipboard(false)
    }

    pub fn cut_selection(&mut self) -> bool {
        // A read-only sheet can be copied from but not cut — a cut would clear
        // the source cells on the next paste. Fall back to a plain copy so the
        // clipboard still works without the destructive `is_cut` flag (#24).
        self.snapshot_clipboard(!self.data.is_read_only())
    }

    /// Paste the clipboard at the current selection's top-left. With multi-range
    /// selection (issue #19), the clipboard lands at the top-left of every
    /// range. A cut clears the source cells afterwards.
    pub fn paste(&mut self) {
        self.paste_special(PasteMode::All);
    }

    /// Paste the clipboard with a specific mode (issue #28). `All` is the
    /// ordinary full-fidelity paste; `Values`/`Formulas`/`Formats` write only a
    /// facet (keeping the destination's other attributes); `Transpose` swaps
    /// rows/columns; `Link` writes `=SourceCell` references. Only `All` consumes
    /// a cut (clears the source); the special modes behave like a copy.
    pub fn paste_special(&mut self, mode: PasteMode) {
        if self.clipboard.is_none() || self.data.is_read_only() {
            return;
        }
        self.snapshot();
        let Some(cb) = self.clipboard.take() else {
            return;
        };
        let src = if mode == PasteMode::Transpose {
            transpose_clipboard(&cb)
        } else {
            cb.clone()
        };
        let destinations: Vec<(usize, usize)> = self
            .selection_ranges()
            .into_iter()
            .map(|(r0, c0, _, _)| (r0, c0))
            .collect();
        for (dr0, dc0) in destinations {
            for (i, row) in src.cells.iter().enumerate() {
                for (j, cell) in row.iter().enumerate() {
                    let (r, c) = (dr0 + i, dc0 + j);
                    if !self.data.is_cell_editable(r, c) {
                        continue;
                    }
                    let value = src
                        .values
                        .get(i)
                        .and_then(|v| v.get(j))
                        .map_or("", |s| s.as_str());
                    let src_ref = crate::renderer::alphabets::xy2expr(src.c0 + j, src.r0 + i);
                    match paste_cell_plan(mode, cell, value, &src_ref) {
                        CellWrite::Full(c2) => self.data.set_cell(r, c, c2),
                        CellWrite::Text(t) => self.data.set_cell_text(r, c, &t),
                        CellWrite::Style(idx) => self.data.set_cell_style(r, c, idx),
                        CellWrite::Skip => {}
                    }
                }
            }
        }
        if cb.is_cut && mode == PasteMode::All {
            for ri in cb.r0..=cb.r1 {
                for ci in cb.c0..=cb.c1 {
                    // Don't clear locked source cells (issue #24).
                    if self.data.is_cell_editable(ri, ci) {
                        self.data.delete_cell(ri, ci);
                    }
                }
            }
        } else {
            self.clipboard = Some(cb); // keep for repeated paste
        }
    }

    /// Whether the in-app clipboard currently holds a snapshot. The system
    /// clipboard glue checks this to decide between a lossless internal paste
    /// and parsing external clipboard content.
    pub fn has_clipboard(&self) -> bool {
        self.clipboard.is_some()
    }

    /// The current selection as a single rectangular range, or `None` for a
    /// non-contiguous (Ctrl+click) multi-range selection that can't be copied
    /// to the system clipboard as one block.
    pub fn contiguous_selection(&self) -> Option<CellRange> {
        if self.multi_range.is_active() {
            return None;
        }
        let (r0, c0, r1, c1) = self.selection_bounds();
        Some(CellRange::new(r0, c0, r1, c1))
    }

    /// Paste a grid parsed from external clipboard content (Excel, Sheets, …)
    /// at the selection's top-left. Writes each cell's text (formulas land as
    /// formulas) honoring per-cell editability, then re-applies merges from the
    /// pasted spans. Snapshots first so the paste is a single undo step.
    pub fn paste_external(&mut self, grid: ParsedGrid) {
        if self.data.is_read_only() || grid.is_empty() {
            return;
        }
        self.snapshot();
        let (r0, c0, _, _) = self.selection_bounds();
        // Clear any existing merge straddling the destination first — pasting
        // over part of a merge must unmerge it, never leave it half-overwritten.
        let width = grid.cells.iter().map(|row| row.len()).max().unwrap_or(0);
        if width > 0 {
            let extent = CellRange::new(r0, c0, r0 + grid.rows().saturating_sub(1), c0 + width - 1);
            self.data.unmerge_intersecting(&extent);
        }
        // First pass: text. Second pass: merges (which clear covered cells, so
        // they must run after every anchor's text is in place).
        for (i, row) in grid.cells.iter().enumerate() {
            for (j, pc) in row.iter().enumerate() {
                let (r, c) = (r0 + i, c0 + j);
                if self.data.is_cell_editable(r, c) {
                    self.data.set_cell_text(r, c, &pc.text);
                }
            }
        }
        for (i, row) in grid.cells.iter().enumerate() {
            for (j, pc) in row.iter().enumerate() {
                if pc.is_merged() {
                    let (r, c) = (r0 + i, c0 + j);
                    let range = CellRange::new(r, c, r + pc.row_span - 1, c + pc.col_span - 1);
                    self.data.merge_range(range);
                }
            }
        }
    }

    pub fn clear_selection_content(&mut self) {
        if self.data.is_read_only() {
            return;
        }
        self.snapshot();
        let cells: Vec<(usize, usize)> = {
            let mut v = Vec::new();
            self.for_each_selected_cell(|ri, ci| v.push((ri, ci)));
            v
        };
        for (ri, ci) in cells {
            if self.data.is_cell_editable(ri, ci) {
                self.data.delete_cell(ri, ci);
            }
        }
    }

    pub fn insert_row_at_selection(&mut self) {
        if self.data.is_read_only() {
            return;
        }
        self.snapshot();
        let (r0, _, _, _) = self.selection_bounds();
        self.data.insert_row(r0, 1);
    }

    pub fn delete_rows_at_selection(&mut self) {
        if self.data.is_read_only() {
            return;
        }
        self.snapshot();
        let (r0, _, r1, _) = self.selection_bounds();
        for _ in r0..=r1 {
            self.data.delete_row(r0);
        }
    }

    pub fn insert_col_at_selection(&mut self) {
        if self.data.is_read_only() {
            return;
        }
        self.snapshot();
        let (_, c0, _, _) = self.selection_bounds();
        self.data.insert_col(c0, 1);
    }

    pub fn delete_cols_at_selection(&mut self) {
        if self.data.is_read_only() {
            return;
        }
        self.snapshot();
        let (_, c0, _, c1) = self.selection_bounds();
        for _ in c0..=c1 {
            self.data.delete_col(c0);
        }
    }

    /// Hide every row spanned by the selection (issue #14).
    pub fn hide_rows_at_selection(&mut self) {
        if self.data.is_read_only() {
            return;
        }
        self.snapshot();
        let (r0, _, r1, _) = self.selection_bounds();
        for r in r0..=r1 {
            self.data.set_row_hidden(r, true);
        }
    }

    /// Reveal every row spanned by the selection — including collapsed rows
    /// caught between the selected ones (issue #14).
    pub fn unhide_rows_at_selection(&mut self) {
        if self.data.is_read_only() {
            return;
        }
        self.snapshot();
        let (r0, _, r1, _) = self.selection_bounds();
        for r in r0..=r1 {
            self.data.set_row_hidden(r, false);
        }
    }

    /// Hide every column spanned by the selection (issue #14).
    pub fn hide_cols_at_selection(&mut self) {
        if self.data.is_read_only() {
            return;
        }
        self.snapshot();
        let (_, c0, _, c1) = self.selection_bounds();
        for c in c0..=c1 {
            self.data.set_col_hidden(c, true);
        }
    }

    /// Reveal every column spanned by the selection (issue #14).
    pub fn unhide_cols_at_selection(&mut self) {
        if self.data.is_read_only() {
            return;
        }
        self.snapshot();
        let (_, c0, _, c1) = self.selection_bounds();
        for c in c0..=c1 {
            self.data.set_col_hidden(c, false);
        }
    }

    /// Insert blank cells over the selection, shifting existing cells right
    /// (`horizontal`) or down (issue #14).
    pub fn insert_cells_at_selection(&mut self, horizontal: bool) {
        if self.data.is_read_only() {
            return;
        }
        self.snapshot();
        let (r0, c0, r1, c1) = self.selection_bounds();
        self.data.insert_cells(r0, c0, r1, c1, horizontal);
    }

    /// Delete the selected cells, pulling later cells left (`horizontal`) or
    /// up (issue #14).
    pub fn delete_cells_at_selection(&mut self, horizontal: bool) {
        if self.data.is_read_only() {
            return;
        }
        self.snapshot();
        let (r0, c0, r1, c1) = self.selection_bounds();
        self.data.delete_cells(r0, c0, r1, c1, horizontal);
    }

    /// Toggle the AutoFilter on the selection (issue #10). A single-cell
    /// selection expands to the sheet's used extent, so the common
    /// "click one cell, hit the filter button" flow covers the whole table.
    /// Toggling off reveals every row the filter hid.
    pub fn toggle_autofilter(&mut self) {
        if self.data.is_read_only() {
            return;
        }
        self.snapshot();
        if self.data.can_autofilter() {
            let (r0, c0, r1, c1) = self.selection_bounds();
            let range = if r0 == r1 && c0 == c1 {
                self.data
                    .used_extent()
                    .map(|(mr, mc)| CellRange::new(0, 0, mr, mc))
                    .unwrap_or_else(|| CellRange::new(r0, c0, r1, c1))
            } else {
                CellRange::new(r0, c0, r1, c1)
            };
            // `autofilter()` reads the data selector — set it explicitly so the
            // range never depends on selector-sync timing.
            self.data.set_selected_range(range);
        }
        self.data.autofilter();
    }

    /// Set (or with `operator == "all"` clear) the value filter on column
    /// `ci` and re-apply row visibility (issue #10).
    pub fn set_column_filter(&mut self, ci: usize, operator: &str, values: Vec<String>) {
        if self.data.is_read_only() {
            return;
        }
        self.snapshot();
        self.data.auto_filter.add_filter(ci, operator, values);
        self.data.apply_filter_visibility();
    }

    /// Sort the filter range by column `ci` (issue #10).
    pub fn sort_filter(&mut self, ci: usize, asc: bool) {
        if self.data.is_read_only() {
            return;
        }
        self.snapshot();
        self.data.sort_filter_range(ci, asc);
    }

    pub fn sort_filter_multi(&mut self, sorts: &[Sort]) {
        if self.data.is_read_only() || sorts.is_empty() {
            return;
        }
        self.snapshot();
        self.data.sort_filter_range_multi(sorts);
    }

    /// Distinct displayed values in filter column `ci` as
    /// `(value, count, currently_included)`, sorted for the dropdown
    /// (issue #10).
    pub fn filter_items(&self, ci: usize) -> Vec<(String, usize, bool)> {
        let af = &self.data.auto_filter;
        let counts = af.items(ci, |r, c| Some(self.data.cell_display_value(r, c)));
        let current = af.get_filter(ci);
        let mut out: Vec<(String, usize, bool)> = counts
            .into_iter()
            .map(|(v, n)| {
                let checked = current.is_none_or(|f| f.includes(&v));
                (v, n, checked)
            })
            .collect();
        out.sort_by(|a, b| crate::core::data_proxy::cmp_cell_values(&a.0, &b.0, true));
        out
    }

    /// Append a conditional-formatting rule (issue #11).
    pub fn add_cond_rule(&mut self, rule: crate::core::cond_format::CondRule) {
        if self.data.is_read_only() {
            return;
        }
        self.snapshot();
        self.data.cond_formats.push(rule);
    }

    /// Remove the conditional-formatting rule at `idx` (issue #11).
    pub fn remove_cond_rule(&mut self, idx: usize) {
        if self.data.is_read_only() {
            return;
        }
        if idx >= self.data.cond_formats.len() {
            return;
        }
        self.snapshot();
        self.data.cond_formats.remove(idx);
    }

    /// Move the rule at `idx` up or down by one position. `dir` is
    /// `+1` for down, `-1` for up. No-op at the boundaries or when
    /// the index is out of range. The reorder affects evaluation
    /// order (the first rule that matches a cell wins), so this
    /// matters in practice. Snapshots for undo.
    pub fn move_cond_rule(&mut self, idx: usize, dir: i32) {
        if self.data.is_read_only() || dir == 0 {
            return;
        }
        let new_idx = match dir.signum() {
            1 => idx + 1,
            -1 => idx.checked_sub(1).unwrap_or(usize::MAX),
            _ => return,
        };
        if new_idx >= self.data.cond_formats.len() {
            return;
        }
        self.snapshot();
        self.data.cond_formats.swap(idx, new_idx);
    }

    /// Append a chart (issue #16).
    pub fn add_chart(&mut self, chart: crate::core::chart::Chart) {
        if self.data.is_read_only() {
            return;
        }
        self.snapshot();
        self.data.charts.push(chart);
    }

    /// Add a new pivot (issue #35). `pt` is the spec; the output is written
    /// to a fresh `DataProxy` and added to `sheets`. The renderer's active
    /// sheet becomes the new output sheet. The previous workbook state is
    /// saved in the unified `undo_stack` (issue #62) so the user can
    /// `undo()` cleanly.
    ///
    /// The spec is appended to the *source* sheet's `pivots` list (held in
    /// the registry, not the renderer's local `self.data`) before the
    /// `set_data` swap, so Refresh and the workbook's `get_data` round-trip
    /// can find it (issue #50). The workbook's active index is updated to
    /// the new output sheet's index so the tab strip highlights the
    /// right tab (issue #54).
    ///
    /// The caller is responsible for re-rendering the tab strip and calling
    /// `sync()`.
    pub fn add_pivot(
        &mut self,
        pt: crate::core::pivot::PivotTable,
        sheets: &SheetsRegistry,
        active: &ActiveSheet,
    ) {
        if self.data.is_read_only() {
            return;
        }
        // 1. Snapshot the current workbook for undo (issue #62). Replaces
        //    the old bespoke `pivot_undo` field — `undo_stack` is now a
        //    uniform `Vec<WorkbookSnapshot>` regardless of the mutator.
        self.snapshot_workbook(sheets, active);
        // 2. Run the aggregation.
        let result = match crate::core::pivot::compute(&self.data, &pt) {
            Ok(r) => r,
            Err(e) => {
                // Drop the undo record — the operation didn't happen.
                self.undo_stack.pop();
                pivot_alert(&format!("Pivot: {}", e));
                return;
            }
        };
        let (r0, c0, _r1, c1) = match pt.validate(&self.data) {
            Ok(b) => b,
            Err(e) => {
                self.undo_stack.pop();
                pivot_alert(&format!("Pivot: {}", e));
                return;
            }
        };
        let headers = crate::core::pivot::read_field_headers(&self.data, r0, c0, c1);
        // 3. Materialize to a new DataProxy, set read-only, push to registry.
        let output_name = pt.output_sheet.clone();
        let mut out_sheet = crate::core::pivot::materialize(&pt, &result, &headers, &output_name);
        out_sheet.set_read_only(true);
        out_sheet.set_sheets(sheets);
        // Phase 5.5: adopt the workbook-wide named-ranges map from
        // an already-wired sibling so the new output sheet can
        // resolve workbook names too.
        let workbook_names = sheets
            .borrow()
            .first()
            .map(|d| std::rc::Rc::clone(&d.workbook_named_ranges))
            .unwrap_or_default();
        out_sheet.link_workbook_named_ranges(workbook_names);
        // 4. Install: spec on the source, output appended/replaced. The spec
        //    must land on the REGISTRY's source entry, not on the renderer's
        //    transient self.data, which is replaced with the output below —
        //    otherwise Refresh and save/load lose the spec (issue #50).
        //    Also updates *active to point at the new output sheet so the
        //    tab strip and the canvas stay in sync (issue #54).
        let new_idx = match install_pivot_into_registry(
            sheets,
            active,
            self.data.clone(),
            pt,
            out_sheet,
        ) {
            Ok(idx) => idx,
            Err(()) => {
                // Source/output name collision (issue #51). The modal
                // should have caught this earlier; defensively refuse.
                self.undo_stack.pop();
                pivot_alert(
                    "Pivot: output sheet name is the same as the source sheet — pick a different name.",
                );
                return;
            }
        };
        // 5. Switch the renderer's active data to the new output.
        let new_data = sheets.borrow()[new_idx].clone();
        self.set_data(new_data);
        self.render();
    }

    /// Re-run the pivot whose `output_sheet` matches the currently active
    /// sheet's name, if any. Called from the "Refresh pivot" context-menu
    /// item on an output sheet (issue #35).
    pub fn refresh_active_pivot(&mut self, sheets: &SheetsRegistry, active: &ActiveSheet) -> bool {
        let active_name = self.data.name.clone();
        // Find the pivot spec on the *source* sheet. We don't know which
        // source sheet it is from the active sheet name alone, so we scan
        // every sheet for a pivot whose `output_sheet` matches.
        let source_idx = sheets
            .borrow()
            .iter()
            .position(|d| d.pivots.iter().any(|p| p.output_sheet == active_name));
        let Some(source_idx) = source_idx else {
            return false;
        };
        let source = sheets.borrow()[source_idx].clone();
        let pt = source
            .pivots
            .iter()
            .find(|p| p.output_sheet == active_name)
            .unwrap()
            .clone();

        // Save the workbook state for undo (issue #62).
        self.snapshot_workbook(sheets, active);

        let result = match crate::core::pivot::compute(&source, &pt) {
            Ok(r) => r,
            Err(e) => {
                self.undo_stack.pop();
                pivot_alert(&format!("Pivot refresh: {}", e));
                return false;
            }
        };
        let (r0, c0, _r1, c1) = match pt.validate(&source) {
            Ok(b) => b,
            Err(e) => {
                self.undo_stack.pop();
                pivot_alert(&format!("Pivot refresh: {}", e));
                return false;
            }
        };
        let headers = crate::core::pivot::read_field_headers(&source, r0, c0, c1);
        let cur = *active.borrow();
        let mut new_sheet =
            crate::core::pivot::materialize(&pt, &result, &headers, &pt.output_sheet);
        new_sheet.set_read_only(true);
        new_sheet.set_sheets(sheets);
        let workbook_names = sheets
            .borrow()
            .first()
            .map(|d| std::rc::Rc::clone(&d.workbook_named_ranges))
            .unwrap_or_default();
        new_sheet.link_workbook_named_ranges(workbook_names);
        sheets.borrow_mut()[cur] = new_sheet;
        self.set_data(sheets.borrow()[cur].clone());
        self.render();
        true
    }

    /// Re-compute every pivot on `source_idx` (issue #61). Used by the
    /// slicer panel: a chip toggle on any slicer may have changed the
    /// "which rows pass" filter for every pivot on the source, so the
    /// cheapest correct response is to re-run them all. Each pivot's
    /// output sheet is replaced in place; the active sheet is
    /// refreshed if it was one of the output sheets.
    pub fn refresh_pivots_on_source(
        &mut self,
        sheets: &SheetsRegistry,
        active: &ActiveSheet,
        source_idx: usize,
    ) {
        let (pivot_specs, source) = {
            let s = sheets.borrow();
            (s[source_idx].pivots.clone(), s[source_idx].clone())
        };
        if pivot_specs.is_empty() {
            return;
        }
        // Snapshot the workbook so the user can undo a slicer change
        // (issue #62). One snapshot covers the whole re-render — the
        // re-render itself isn't undoable step-by-step, only the
        // before/after state is.
        self.snapshot_workbook(sheets, active);

        // Recompute each pivot. We collect the updates first so the
        // Sheets borrow can drop between compute and apply.
        let mut updates: Vec<(String, DataProxy)> = Vec::with_capacity(pivot_specs.len());
        for pt in pivot_specs.iter() {
            let result = match crate::core::pivot::compute(&source, pt) {
                Ok(r) => r,
                Err(e) => {
                    // A single bad pivot shouldn't kill the whole batch.
                    // We just skip it — the next successful refresh will
                    // re-try. Log the error via the same path Refresh uses.
                    crate::zedsheet::pivot_alert(&format!("Pivot: {}", e));
                    continue;
                }
            };
            let (r0, c0, _r1, c1) = match pt.validate(&source) {
                Ok(b) => b,
                Err(_) => continue,
            };
            let headers = crate::core::pivot::read_field_headers(&source, r0, c0, c1);
            let mut out = crate::core::pivot::materialize(pt, &result, &headers, &pt.output_sheet);
            out.set_read_only(true);
            out.set_sheets(sheets);
            let workbook_names = sheets
                .borrow()
                .first()
                .map(|d| std::rc::Rc::clone(&d.workbook_named_ranges))
                .unwrap_or_default();
            out.link_workbook_named_ranges(workbook_names);
            updates.push((pt.output_sheet.clone(), out));
        }

        // Apply updates: replace each output sheet in the registry. We
        // scan by name to handle the case where a pivot's output is
        // outside the source_idx sheet (it always is — the output is
        // its own sheet, possibly the active one).
        let active_name = sheets.borrow()[*active.borrow()].name.clone();
        let mut active_was_replaced = false;
        {
            let mut s = sheets.borrow_mut();
            for (name, new_sheet) in updates.iter() {
                if let Some(idx) = s.iter().position(|d| d.name == *name) {
                    s[idx] = new_sheet.clone();
                    if name == &active_name {
                        active_was_replaced = true;
                    }
                }
            }
        }

        // Refresh the renderer's view of the active sheet if we just
        // replaced it (it's a pivot output that's been recomputed).
        if active_was_replaced {
            self.set_data(sheets.borrow()[*active.borrow()].clone());
        }
        self.render();
    }

    /// Remove the chart at `idx` (issue #16).
    pub fn remove_chart(&mut self, idx: usize) {
        if self.data.is_read_only() {
            return;
        }
        if idx >= self.data.charts.len() {
            return;
        }
        self.snapshot();
        self.data.charts.remove(idx);
    }

    /// If `(x, y)` hits the dropdown glyph on an AutoFilter header cell, the
    /// column index (issue #10). The glyph occupies the rightmost ~17px.
    pub fn filter_glyph_hit(&self, x: f64, y: f64) -> Option<usize> {
        let hrange = self.data.auto_filter.hrange()?;
        let (ri, ci) = self.cell_at(x, y)?;
        if !hrange.includes(ri, ci) {
            return None;
        }
        let rect = self.cell_screen_rect(ri, ci);
        let in_glyph = x >= rect.x + rect.width - 17.0
            && x <= rect.x + rect.width
            && y >= rect.y
            && y <= rect.y + rect.height;
        in_glyph.then_some(ci)
    }

    /// Set the freeze origin (rows above `ri` and columns left of `ci` stay
    /// fixed), keeping the renderer and the data model in sync so the freeze
    /// persists across serialization and sheet switches (issue #18).
    fn set_freeze_origin(&mut self, ri: usize, ci: usize) {
        self.freeze = (ri, ci);
        self.data.set_freeze(ri, ci);
    }

    /// Freeze the top row (issue #18).
    pub fn freeze_top_row(&mut self) {
        self.set_freeze_origin(1, 0);
    }

    /// Freeze the first column (issue #18).
    pub fn freeze_first_col(&mut self) {
        self.set_freeze_origin(0, 1);
    }

    /// Freeze the rows above and columns left of the active cell (issue #18).
    pub fn freeze_at_selection(&mut self) {
        let (r0, c0, _, _) = self.selection_bounds();
        self.set_freeze_origin(r0, c0);
    }

    /// Remove all frozen panes (issue #18).
    pub fn unfreeze(&mut self) {
        self.set_freeze_origin(0, 0);
    }

    /// Toggle the per-cell lock (editable flag) on the active cell. Snapshots
    /// first so the lock change is itself undoable — otherwise undoing an
    /// unrelated earlier edit would silently restore the old lock state (#24).
    pub fn toggle_selection_editable(&mut self) {
        if self.data.is_read_only() {
            return;
        }
        self.snapshot();
        let (r0, c0, _, _) = self.selection_bounds();
        let was = self
            .data
            .get_cell(r0, c0)
            .map(|c| c.editable)
            .unwrap_or(true);
        self.data.set_cell_editable(r0, c0, !was);
    }

    pub fn cell_text_at(&self, ri: usize, ci: usize) -> String {
        self.data.get_cell_text(ri, ci)
    }

    pub fn note_at(&self, ri: usize, ci: usize) -> Option<String> {
        self.data.get_note(ri, ci)
    }

    /// Set or clear the note on the active cell (selection top-left), with undo.
    pub fn set_selection_note(&mut self, note: Option<String>) {
        if self.data.is_read_only() {
            return;
        }
        self.snapshot();
        let (r0, c0, _, _) = self.selection_bounds();
        self.data.set_note(r0, c0, note);
    }

    /// The note for the active cell (selection top-left), if any.
    pub fn selection_note(&self) -> Option<String> {
        let (r0, c0, _, _) = self.selection_bounds();
        self.data.get_note(r0, c0)
    }

    pub fn link_at(&self, ri: usize, ci: usize) -> Option<String> {
        self.data.get_link(ri, ci)
    }

    /// Set or clear the hyperlink on the active cell, with undo. A raw target is
    /// normalized via [`crate::core::link::normalize_link`] (blank clears it).
    pub fn set_selection_link(&mut self, link: Option<String>) {
        if self.data.is_read_only() {
            return;
        }
        self.snapshot();
        let (r0, c0, _, _) = self.selection_bounds();
        let normalized = link.and_then(|s| crate::core::link::normalize_link(&s));
        self.data.set_link(r0, c0, normalized);
    }

    /// The hyperlink for the active cell (selection top-left), if any.
    pub fn selection_link(&self) -> Option<String> {
        let (r0, c0, _, _) = self.selection_bounds();
        self.data.get_link(r0, c0)
    }

    pub fn get_selector(&self) -> SelectorRect {
        self.selector
    }

    // --- Named ranges (issue #21) ---

    /// The range expression (e.g. `"B2:B3"`) for a defined name, if any.
    pub fn named_range_expr(&self, name: &str) -> Option<String> {
        self.data.get_named_range(name)
    }

    /// Define a named range covering the current selection (undoable). The range
    /// expression is the selection's bounds in `A1` / `A1:B3` form.
    pub fn define_selection_name(&mut self, name: &str) {
        self.snapshot();
        let (r0, c0, r1, c1) = self.selection_bounds();
        let expr = if r0 == r1 && c0 == c1 {
            crate::renderer::alphabets::xy2expr(c0, r0)
        } else {
            format!(
                "{}:{}",
                crate::renderer::alphabets::xy2expr(c0, r0),
                crate::renderer::alphabets::xy2expr(c1, r1)
            )
        };
        self.data.set_named_range(name, &expr);
    }

    /// Select the cells of a defined name; returns whether the name existed.
    pub fn select_named(&mut self, name: &str) -> bool {
        match self.data.named_range_bounds(name) {
            Some((r0, c0, r1, c1)) => {
                self.select_cell(r0, c0);
                self.select_to(r1, c1);
                true
            }
            None => false,
        }
    }

    // --- Fill handle (issue #12) ---

    /// Record the fill handle's screen rect during rendering (for hit-testing).
    pub fn set_fill_handle_rect(&self, rect: Option<(f64, f64, f64, f64)>) {
        self.last_fill_handle.set(rect);
    }

    /// Whether the point `(x, y)` is on the selection's fill handle.
    pub fn is_on_fill_handle(&self, x: f64, y: f64) -> bool {
        match self.last_fill_handle.get() {
            // A small tolerance makes the 6px handle easier to grab.
            Some((hx, hy, hw, hh)) => {
                let t = 2.0;
                x >= hx - t && x <= hx + hw + t && y >= hy - t && y <= hy + hh + t
            }
            None => false,
        }
    }

    /// Begin a fill-handle drag, recording the current selection as the source.
    pub fn start_fill(&mut self) {
        self.fill_source = Some(self.selection_bounds());
    }

    /// Apply the fill from the recorded source into the (drag-extended) selection,
    /// copying values/formats and continuing numeric series. Undoable.
    pub fn apply_fill(&mut self) {
        let Some((sr0, sc0, sr1, sc1)) = self.fill_source.take() else {
            return;
        };
        // Read-only sheets reject fill (issue #24); fill_source already cleared.
        if self.data.is_read_only() {
            return;
        }
        let (_, _, tr1, tc1) = self.selection_bounds();
        let down = tr1 as isize - sr1 as isize;
        let right = tc1 as isize - sc1 as isize;
        if down <= 0 && right <= 0 {
            return; // dragged back inside the source — nothing to fill
        }
        self.snapshot();
        if down >= right {
            let n = down as usize;
            for c in sc0..=sc1 {
                let source: Vec<String> =
                    (sr0..=sr1).map(|r| self.data.get_cell_text(r, c)).collect();
                let styles: Vec<Option<usize>> = (sr0..=sr1)
                    .map(|r| self.data.get_cell(r, c).and_then(|cell| cell.style))
                    .collect();
                let filled = crate::core::data_proxy::fill_line(&source, n, true);
                let slen = source.len();
                for (i, text) in filled.iter().enumerate() {
                    let tr = sr1 + 1 + i;
                    if !self.data.is_cell_editable(tr, c) {
                        continue; // skip locked target cells (issue #24)
                    }
                    self.data.set_cell_text(tr, c, text);
                    // Fill replaces the target's format with the source cell's.
                    self.data.get_cell_or_new(tr, c).style = styles[i % slen];
                }
            }
        } else {
            let n = right as usize;
            for r in sr0..=sr1 {
                let source: Vec<String> =
                    (sc0..=sc1).map(|c| self.data.get_cell_text(r, c)).collect();
                let styles: Vec<Option<usize>> = (sc0..=sc1)
                    .map(|c| self.data.get_cell(r, c).and_then(|cell| cell.style))
                    .collect();
                let filled = crate::core::data_proxy::fill_line(&source, n, false);
                let slen = source.len();
                for (i, text) in filled.iter().enumerate() {
                    let tc = sc1 + 1 + i;
                    if !self.data.is_cell_editable(r, tc) {
                        continue; // skip locked target cells (issue #24)
                    }
                    self.data.set_cell_text(r, tc, text);
                    // Fill replaces the target's format with the source cell's.
                    self.data.get_cell_or_new(r, tc).style = styles[i % slen];
                }
            }
        }
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

    /// Resize a column from a header drag. `w` is in SCREEN pixels (the drag
    /// captures `col_width_at`, which is zoomed, and adds a cursor delta), so
    /// divide by zoom to store the model width (issue #32).
    pub fn set_col_width_clamped(&mut self, ci: usize, w: f64) {
        let model_w = w / self.data.zoom();
        self.data.set_col_width(ci, model_w.max(20f64));
    }

    pub fn set_row_height_clamped(&mut self, ri: usize, h: f64) {
        let model_h = h / self.data.zoom();
        self.data.set_row_height(ri, model_h.max(12f64));
    }

    /// Auto-fit a column's width to its content: measure every visible cell in
    /// the column and set the width to the widest one plus padding.
    pub fn auto_fit_col(&mut self, ci: usize) {
        let canvas = Canvas::new(self.target.clone(), self.scale);
        let Some((max_r, _)) = self.data.used_extent() else {
            return;
        };
        let mut max_w: f64 = 60.0; // minimum column width in model pixels
        for ri in 0..=max_r {
            if self.data.is_row_hidden(ri) {
                continue;
            }
            let display = self.data.cell_display_value(ri, ci);
            if display.is_empty() {
                continue;
            }
            let style = self.data.get_cell_style(ri, ci);
            let font_size = style.font_size as f64;
            let bold = if style.bold { "bold " } else { "" };
            let italic = if style.italic { "italic " } else { "" };
            let font = format!(
                "{}{}{}px {}",
                bold,
                italic,
                font_size + 2.0,
                style.font_family
            );
            canvas.ctx.set_font(&font);
            let tw = canvas.measure_text_width(&display);
            // Wrap-aware: if cell has text_wrap, measure each line
            let cell_w = if style.text_wrap {
                let pad = 10.0; // 5px each side
                let col_w = self.data.get_col_width(ci) / self.data.zoom();
                let lines = crate::renderer::render::wrap_text(&canvas, &display, col_w - pad);
                lines
                    .iter()
                    .map(|l| canvas.measure_text_width(l))
                    .fold(0f64, f64::max)
            } else {
                tw
            };
            let needed = cell_w + 12.0; // 6px padding each side
            if needed > max_w {
                max_w = needed;
            }
        }
        self.data.set_col_width(ci, max_w.round().max(20.0));
    }

    /// Auto-fit a row's height to its content: measure every visible cell in
    /// the row and set the height to accommodate the tallest one.
    pub fn auto_fit_row(&mut self, ri: usize) {
        let canvas = Canvas::new(self.target.clone(), self.scale);
        let (_max_r, max_c) = self.data.used_extent().unwrap_or((0, 0));
        let mut max_h: f64 = 12.0; // minimum row height in model pixels
        for ci in 0..=max_c {
            if self.data.is_col_hidden(ci) {
                continue;
            }
            let display = self.data.cell_display_value(ri, ci);
            if display.is_empty() {
                continue;
            }
            let style = self.data.get_cell_style(ri, ci);
            let font_size = style.font_size as f64;
            let font = format!("{}px {}", font_size + 2.0, style.font_family);
            canvas.ctx.set_font(&font);
            if style.text_wrap {
                let pad = 10.0;
                let col_w = self.data.get_col_width(ci) / self.data.zoom();
                let lines = crate::renderer::render::wrap_text(&canvas, &display, col_w - pad);
                let line_h = font_size + 6.0;
                let needed = lines.len() as f64 * line_h + 8.0;
                if needed > max_h {
                    max_h = needed;
                }
            } else {
                let needed = font_size + 14.0;
                if needed > max_h {
                    max_h = needed;
                }
            }
        }
        self.data.set_row_height(ri, max_h.round().max(12.0));
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

/// Show a JS alert dialog (issue #35 pivot errors). Standalone helper to
/// avoid a `zedsheet::util` import cycle from the renderer.
fn pivot_alert(msg: &str) {
    if let Some(win) = web_sys::window() {
        let _ = win.alert_with_message(msg);
    }
}

/// Install a freshly-materialized pivot output into the workbook's sheet
/// registry (issue #35 / #50). The pivot spec is appended to the source
/// sheet's `pivots` list so the spec survives workbook round-trip and is
/// discoverable by Refresh. Returns the registry index of the new (or
/// replaced) output sheet.
///
/// `source` is the source `DataProxy` snapshot the caller was looking at
/// when Create was clicked (the renderer's active data). The function does
/// NOT read the source from `sheets[cur]` — passing it in keeps the spec
/// attached to the caller's view, even if `cur` is stale relative to the
/// registry (issue #52, separate fix).
///
/// Reads the *current* active index from `active` (not the caller's snapshot)
/// so a tab switch between opening the modal and clicking Create pivot
/// routes the pivot against the sheet the user is actually on, and writes
/// the new output sheet's index back into `active` so the tab strip and
/// the canvas stay in sync (issue #54).
///
/// Returns the index of the new (or replaced) output sheet. Returns
/// `Err(())` if the user tried to use the source sheet's name as the
/// output (issue #51) — the caller drops the pivot_undo record and
/// shows an alert.
fn install_pivot_into_registry(
    sheets: &SheetsRegistry,
    active: &ActiveSheet,
    source: DataProxy,
    pt: crate::core::pivot::PivotTable,
    out_sheet: DataProxy,
) -> Result<usize, ()> {
    let cur = *active.borrow();
    let output_name = out_sheet.name.clone();
    // Defensively refuse to clobber the active source sheet (issue #51).
    if let Some(existing_idx) = sheets.borrow().iter().position(|d| d.name == output_name) {
        if existing_idx == cur {
            return Err(());
        }
    }
    // Build a source snapshot WITH the spec appended. This is the entry
    // that gets written to s[cur] so Refresh can find it. Pushing onto
    // self.data instead would lose the spec at the very next set_data
    // (which replaces self.data with the output sheet) — that was the bug
    // in add_pivot before this fix (issue #50).
    let mut updated_source = source;
    updated_source.pivots.push(pt);
    let new_idx = {
        let mut s = sheets.borrow_mut();
        if let Some(existing_idx) = s.iter().position(|d| d.name == output_name) {
            s[cur] = updated_source;
            s[existing_idx] = out_sheet;
            existing_idx
        } else {
            s[cur] = updated_source;
            s.push(out_sheet);
            s.len() - 1
        }
    };
    // Update the workbook's active index so the tab strip and the canvas
    // agree (issue #54).
    *active.borrow_mut() = new_idx;
    Ok(new_idx)
}

/// Map a canvas pixel `p` (already past the `header` gutter) to a track
/// (row or column) index, accounting for frozen panes.
///
/// Frozen panes split an axis the way [`Viewport`] lays its areas out: the
/// pinned band `[start, frozen)` is rendered flush against `header`, and the
/// scrolled body (`body_start..total`) is rendered immediately *after* the
/// pinned band's total extent. Hit-testing therefore has to walk the pinned
/// band first and only step into the body once `p` is past it — walking the
/// body straight from `header` (ignoring the pinned extent) is what made every
/// body click land one frozen row/column too far (the frozen-header off-by-one).
///
/// `size(i)` returns track `i`'s height/width (0 for hidden tracks, which are
/// skipped naturally). The result is clamped to the last valid index.
fn track_at(
    p: f64,
    header: f64,
    start: usize,
    frozen: usize,
    body_start: usize,
    total: usize,
    size: impl Fn(usize) -> f64,
) -> usize {
    let frozen_end = frozen.max(start);
    let mut frozen_extent = 0f64;
    for i in start..frozen_end {
        frozen_extent += size(i);
    }

    // Pinned band when `p` falls within the frozen extent, otherwise the body.
    let (mut pos, mut idx, end) = if p < header + frozen_extent {
        (header, start, frozen_end)
    } else {
        (header + frozen_extent, body_start, total)
    };

    while idx < end {
        let s = size(idx);
        if p < pos + s {
            break;
        }
        pos += s;
        idx += 1;
    }
    idx.min(total.saturating_sub(1))
}

/// Leading-edge pixel offset of track `idx` along one axis — the inverse of
/// [`track_at`], using the same frozen-pane layout so a cell's drawn rect and
/// its hit-test agree under freeze.
fn track_offset(
    idx: usize,
    header: f64,
    start: usize,
    frozen: usize,
    body_start: usize,
    size: impl Fn(usize) -> f64,
) -> f64 {
    let frozen_end = frozen.max(start);
    if idx < frozen_end {
        let mut pos = header;
        for i in start..idx {
            pos += size(i);
        }
        pos
    } else {
        let mut pos = header;
        for i in start..frozen_end {
            pos += size(i);
        }
        for i in body_start..idx {
            pos += size(i);
        }
        pos
    }
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
    use super::{
        install_pivot_into_registry, paste_cell_plan, replace_ci, track_at, track_offset,
        transpose_clipboard, CellWrite, ClipboardData, DataCell, PasteMode,
    };
    use crate::core::data_proxy::{DataProxy, SheetsRegistry};
    use crate::core::pivot::{compute, materialize, read_field_headers, Agg, PivotTable};
    use std::cell::RefCell;
    use std::rc::Rc;

    /// Build a registry with one source sheet holding the given rows and wire
    /// every sheet's back-reference to it. Returns the source DataProxy and
    /// the registry; the caller must hold the registry Rc alive.
    fn source_only_workbook(name: &str, rows: &[&[&str]]) -> (DataProxy, SheetsRegistry) {
        let mut src = DataProxy::new(name);
        for (ri, row) in rows.iter().enumerate() {
            for (ci, cell) in row.iter().enumerate() {
                if !cell.is_empty() {
                    src.set_cell_text(ri, ci, cell);
                }
            }
        }
        let sheets: SheetsRegistry = Rc::new(RefCell::new(vec![src.clone()]));
        for d in sheets.borrow_mut().iter_mut() {
            d.set_sheets(&sheets);
        }
        src.set_sheets(&sheets);
        // Phase 5.5: with a single sheet, the lone DataProxy
        // already owns its Rc — the link is a no-op when the
        // pointers are already equal.
        let workbook_names = std::rc::Rc::clone(&src.workbook_named_ranges);
        src.link_workbook_named_ranges(workbook_names);
        (src, sheets)
    }

    /// Build a pivot spec with the given source range and field layout.
    fn pt_for(
        source_range: &str,
        source_sheet: &str,
        output_sheet: &str,
        row_fields: Vec<usize>,
        col_fields: Vec<usize>,
        value_field: usize,
        agg: Agg,
    ) -> PivotTable {
        PivotTable {
            source_range: source_range.into(),
            source_sheet: source_sheet.into(),
            row_fields,
            col_fields,
            value_field,
            agg,
            value_fields: vec![],
            filter_fields: vec![],
            date_groups: std::collections::HashMap::new(),
            output_sheet: output_sheet.into(),
        }
    }

    #[test]
    fn install_pivot_puts_spec_on_registry_source() {
        // Issue #50: the spec must land on the source sheet in the registry,
        // not on the renderer's transient self.data — otherwise Refresh and
        // workbook round-trip can't find it.
        let (src, sheets) = source_only_workbook(
            "Sales",
            &[
                &["Region", "Amount"],
                &["North", "100"],
                &["North", "200"],
                &["South", "50"],
            ],
        );

        let pt = pt_for(
            "Sales!A1:B4",
            "Sales",
            "Pivot1",
            vec![0],
            vec![],
            1,
            Agg::Sum,
        );
        let result = compute(&src, &pt).unwrap();
        let headers = read_field_headers(&src, 0, 0, 1);
        let mut out = materialize(&pt, &result, &headers, "Pivot1");
        out.set_read_only(true);
        out.set_sheets(&sheets);

        let new_idx = install_pivot_into_registry(
            &sheets,
            &Rc::new(RefCell::new(0)),
            src.clone(),
            pt.clone(),
            out,
        );

        // The source in the registry has the spec, so Refresh / save/load work.
        let new_idx = new_idx.expect("install should succeed");
        let after = sheets.borrow();
        assert_eq!(after.len(), 2, "output should be appended");
        assert_eq!(new_idx, 1);
        assert_eq!(after[0].pivots.len(), 1, "spec must land on the source");
        assert_eq!(after[0].pivots[0].output_sheet, "Pivot1");
        assert_eq!(after[0].pivots[0].row_fields, vec![0]);
        assert_eq!(after[0].pivots[0].value_field, 1);
        assert_eq!(after[0].pivots[0].agg, Agg::Sum);
        // Source's cells and name are otherwise unchanged.
        assert_eq!(after[0].name, "Sales");
        assert_eq!(after[0].get_cell_text(1, 0), "North");
        // Output lands at new_idx.
        assert_eq!(after[new_idx].name, "Pivot1");
        assert!(after[new_idx].is_read_only());
    }

    #[test]
    fn install_pivot_replaces_existing_output() {
        // The output name already exists in the registry (a different sheet
        // than the source). The function overwrites that sheet in place and
        // still appends the spec to the source.
        let (mut src, sheets) = source_only_workbook(
            "Sales",
            &[&["Region", "Amount"], &["North", "100"], &["South", "50"]],
        );
        // Add a stale "Pivot1" sheet at index 1 that should be overwritten.
        let stale = DataProxy::new("Pivot1");
        sheets.borrow_mut().push(stale);
        for d in sheets.borrow_mut().iter_mut() {
            d.set_sheets(&sheets);
        }
        src.set_sheets(&sheets);
        // Phase 5.5: link the workbook-wide named-ranges map across
        // all sheets so workbook names defined on `src` are visible
        // from the stale sheet too.
        let workbook_names = std::rc::Rc::clone(&src.workbook_named_ranges);
        for d in sheets.borrow_mut().iter_mut() {
            d.link_workbook_named_ranges(std::rc::Rc::clone(&workbook_names));
        }

        let pt = pt_for(
            "Sales!A1:B3",
            "Sales",
            "Pivot1",
            vec![0],
            vec![],
            1,
            Agg::Sum,
        );
        let result = compute(&src, &pt).unwrap();
        let headers = read_field_headers(&src, 0, 0, 1);
        let mut out = materialize(&pt, &result, &headers, "Pivot1");
        out.set_read_only(true);
        out.set_sheets(&sheets);

        let new_idx = install_pivot_into_registry(
            &sheets,
            &Rc::new(RefCell::new(0)),
            src.clone(),
            pt.clone(),
            out,
        )
        .expect("install should succeed");

        let after = sheets.borrow();
        assert_eq!(
            after.len(),
            2,
            "no append — the existing sheet was replaced"
        );
        assert_eq!(new_idx, 1);
        assert_eq!(after[0].pivots.len(), 1, "source still gets the spec");
        assert_eq!(after[1].name, "Pivot1");
        assert!(after[1].is_read_only());
        // The output should hold the fresh materialization, not the stale one.
        assert_eq!(after[1].get_cell_text(0, 0), "Region");
    }

    #[test]
    fn install_pivot_does_not_mutate_caller_source() {
        // The helper receives `source` by value but should not modify the
        // caller's clone — the caller's view of the source is independent
        // of the registry's view. This matters for the renderer's
        // pivot_undo record, which keeps its own pre-pivot snapshot.
        let (src, sheets) = source_only_workbook(
            "Sales",
            &[&["Region", "Amount"], &["North", "100"], &["South", "50"]],
        );
        let src_pivots_before = src.pivots.len();

        let pt = pt_for(
            "Sales!A1:B3",
            "Sales",
            "Pivot1",
            vec![0],
            vec![],
            1,
            Agg::Sum,
        );
        let result = compute(&src, &pt).unwrap();
        let headers = read_field_headers(&src, 0, 0, 1);
        let out = materialize(&pt, &result, &headers, "Pivot1");

        let _ =
            install_pivot_into_registry(&sheets, &Rc::new(RefCell::new(0)), src.clone(), pt, out);

        // Caller's source is unchanged — the registry got a separate, spec-
        // bearing clone.
        assert_eq!(src.pivots.len(), src_pivots_before);
    }

    #[test]
    fn install_pivot_routes_to_the_active_index_it_was_given() {
        // Issue #52: wire_pivot_modal used to capture `active: usize` at
        // wire time, so if the user switched tabs between Open and Create
        // pivot the spec was routed against the wrong source. The fix
        // makes `active` an `Rc<RefCell<usize>>` that the save handler
        // re-reads. This test documents the model contract: the install
        // helper, called with the *current* (live) active value, routes
        // the spec to that sheet — not to a stale one.
        //
        // We simulate the exact shape of the bug: the workbook has two
        // sheets, the user opens the modal while on Sheet1 (active=0),
        // switches to Sheet2 (active=1), then clicks Create. The install
        // helper must put the spec on Sheet2.
        let (src1, sheets) = source_only_workbook(
            "Sheet1",
            &[&["Region", "Amount"], &["North", "100"], &["South", "50"]],
        );
        // Add a second sheet at index 1 with its own data, so each pivot
        // has a real source to aggregate against.
        let mut src2 = DataProxy::new("Sheet2");
        src2.set_cell_text(0, 0, "Item");
        src2.set_cell_text(0, 1, "Qty");
        src2.set_cell_text(1, 0, "A");
        src2.set_cell_text(1, 1, "7");
        src2.set_cell_text(2, 0, "B");
        src2.set_cell_text(2, 1, "3");
        sheets.borrow_mut().push(src2.clone());
        for d in sheets.borrow_mut().iter_mut() {
            d.set_sheets(&sheets);
        }
        let active: Rc<RefCell<usize>> = Rc::new(RefCell::new(0));

        // First pivot: active=0 → spec lands on Sheet1.
        let pt1 = pt_for(
            "Sheet1!A1:B3",
            "Sheet1",
            "P_from_1",
            vec![0],
            vec![],
            1,
            Agg::Sum,
        );
        let r1 = compute(&src1, &pt1).unwrap();
        let h1 = read_field_headers(&src1, 0, 0, 1);
        let out1 = materialize(&pt1, &r1, &h1, "P_from_1");
        let _ = install_pivot_into_registry(&sheets, &active, src1.clone(), pt1, out1);

        // User switches tabs — the wiring re-reads the live active.
        *active.borrow_mut() = 1;

        // Second pivot: active=1 → spec lands on Sheet2.
        let src2 = sheets.borrow()[1].clone();
        let pt2 = pt_for(
            "Sheet2!A1:B3",
            "Sheet2",
            "P_from_2",
            vec![0],
            vec![],
            1,
            Agg::Sum,
        );
        let r2 = compute(&src2, &pt2).unwrap();
        let h2 = read_field_headers(&src2, 0, 0, 1);
        let out2 = materialize(&pt2, &r2, &h2, "P_from_2");
        let _ = install_pivot_into_registry(&sheets, &active, src2, pt2, out2);

        // Each sheet has its own spec, routed by the active value the
        // helper was given at the time of the call. A stale `active=0`
        // (the original bug) would have overwritten Sheet1 with Sheet2's
        // spec and lost Sheet1's.
        let after = sheets.borrow();
        assert_eq!(after[0].pivots.len(), 1);
        assert_eq!(after[0].pivots[0].output_sheet, "P_from_1");
        assert_eq!(after[1].pivots.len(), 1);
        assert_eq!(after[1].pivots[0].output_sheet, "P_from_2");
    }

    #[test]
    fn paste_cell_plan_picks_the_right_write_per_mode() {
        let mut cell = DataCell::with_text("=A1+1");
        cell.style = Some(3);
        assert!(matches!(
            paste_cell_plan(PasteMode::All, &cell, "42", "A1"),
            CellWrite::Full(_)
        ));
        assert!(matches!(
            paste_cell_plan(PasteMode::Transpose, &cell, "42", "A1"),
            CellWrite::Full(_)
        ));
        match paste_cell_plan(PasteMode::Values, &cell, "42", "A1") {
            CellWrite::Text(t) => assert_eq!(t, "42"), // computed value, not the formula
            w => panic!("{w:?}"),
        }
        match paste_cell_plan(PasteMode::Formulas, &cell, "42", "A1") {
            CellWrite::Text(t) => assert_eq!(t, "=A1+1"),
            w => panic!("{w:?}"),
        }
        match paste_cell_plan(PasteMode::Formats, &cell, "42", "A1") {
            CellWrite::Style(idx) => assert_eq!(idx, 3),
            w => panic!("{w:?}"),
        }
        match paste_cell_plan(PasteMode::Link, &cell, "42", "B7") {
            CellWrite::Text(t) => assert_eq!(t, "=B7"),
            w => panic!("{w:?}"),
        }
        // Formats with no source style leaves the destination untouched.
        let plain = DataCell::with_text("x");
        assert!(matches!(
            paste_cell_plan(PasteMode::Formats, &plain, "", ""),
            CellWrite::Skip
        ));
    }

    #[test]
    fn transpose_clipboard_swaps_grid_and_merge_spans() {
        // 2 rows × 3 cols; anchor (0,0) is a 2×3 merge → span (extra_rows=1, extra_cols=2).
        let mut cells = Vec::new();
        let mut values = Vec::new();
        for i in 0..2 {
            let mut row = Vec::new();
            let mut vrow = Vec::new();
            for j in 0..3 {
                let mut c = DataCell::with_text(&format!("r{i}c{j}"));
                if i == 0 && j == 0 {
                    c.merge = Some((1, 2));
                }
                row.push(c);
                vrow.push(format!("v{i}{j}"));
            }
            cells.push(row);
            values.push(vrow);
        }
        let cb = ClipboardData {
            r0: 0,
            c0: 0,
            r1: 1,
            c1: 2,
            cells,
            values,
            is_cut: false,
        };
        let t = transpose_clipboard(&cb);
        assert_eq!(t.cells.len(), 3); // 3 rows now
        assert_eq!(t.cells[0].len(), 2); // 2 cols now
        assert_eq!(t.cells[2][1].text, "r1c2"); // [1][2] → [2][1]
        assert_eq!(t.values[2][1], "v12");
        assert_eq!(t.cells[0][0].merge, Some((2, 1))); // span swapped
    }

    #[test]
    fn merge_button_toggles_existing_merge() {
        use super::{merge_toggle_action, MergeAction};
        // Multi-cell selection, no merge at its origin → merge it.
        assert_eq!(merge_toggle_action((1, 7, 2, 8), None), MergeAction::Merge);
        // Clicking a merged cell expands the selection to the merge bounds, so
        // selection == merge → unmerge (the bug: this was previously a no-op).
        assert_eq!(
            merge_toggle_action((1, 7, 2, 8), Some((1, 7, 2, 8))),
            MergeAction::Unmerge
        );
        // Single non-merged cell → nothing to do.
        assert_eq!(merge_toggle_action((3, 3, 3, 3), None), MergeAction::NoOp);
        // Selection larger than a merge anchored at its origin → grow (merge),
        // not unmerge.
        assert_eq!(
            merge_toggle_action((0, 0, 3, 3), Some((0, 0, 1, 1))),
            MergeAction::Merge
        );
    }

    #[test]
    fn case_insensitive_replace() {
        assert_eq!(replace_ci("Hello hello HELLO", "hello", "hi"), "hi hi hi");
        assert_eq!(replace_ci("abcabc", "bc", "X"), "aXaX");
        assert_eq!(replace_ci("nothing", "zzz", "Y"), "nothing");
        assert_eq!(replace_ci("keep", "", "x"), "keep");
    }

    // Geometry for the cases below: 20px header, uniform 25px tracks, 100 total.
    const HDR: f64 = 20.0;
    fn sz(_: usize) -> f64 {
        25.0
    }

    #[test]
    fn track_at_no_freeze() {
        // body starts flush against the header.
        let at = |p: f64| track_at(p, HDR, 0, 0, 0, 100, sz);
        assert_eq!(at(20.0), 0); // top edge of row 0
        assert_eq!(at(30.0), 0);
        assert_eq!(at(44.9), 0);
        assert_eq!(at(45.0), 1); // top edge of row 1
        assert_eq!(at(506.0), 19);
        assert_eq!(at(99999.0), 99); // clamps to last
    }

    #[test]
    fn track_at_one_frozen_row() {
        // freeze=1: pinned row 0 occupies [20,45); body (row 1+) starts at 45.
        // Regression: before the fix these all returned one row too far.
        let at = |p: f64| track_at(p, HDR, 0, 1, 1, 100, sz);
        assert_eq!(at(30.0), 0); // inside the pinned row -> row 0
        assert_eq!(at(44.9), 0);
        assert_eq!(at(45.0), 1); // first body row, NOT row 2
        assert_eq!(at(55.0), 1);
        assert_eq!(at(80.0), 2);
        assert_eq!(at(506.0), 19); // was 20 before the fix
    }

    #[test]
    fn track_at_two_frozen_rows() {
        // freeze=2: pinned rows 0,1 occupy [20,70); body (row 2+) starts at 70.
        let at = |p: f64| track_at(p, HDR, 0, 2, 2, 100, sz);
        assert_eq!(at(30.0), 0);
        assert_eq!(at(60.0), 1);
        assert_eq!(at(70.0), 2); // first body row
        assert_eq!(at(95.0), 3);
    }

    #[test]
    fn track_offset_inverts_track_at_under_freeze() {
        // A cell's drawn offset must match where track_at maps its top edge.
        for &(frozen, body_start) in &[(0usize, 0usize), (1, 1), (2, 2)] {
            for idx in 0..40usize {
                let off = track_offset(idx, HDR, 0, frozen, body_start, sz);
                assert_eq!(
                    track_at(off, HDR, 0, frozen, body_start, 100, sz),
                    idx,
                    "freeze={frozen} idx={idx} off={off}"
                );
            }
        }
    }

    #[test]
    fn track_offset_frozen_row_layout() {
        // freeze=1: body row 5 is drawn at 20 + 25(frozen) + 4*25 = 145.
        assert_eq!(track_offset(5, HDR, 0, 1, 1, sz), 145.0);
        // the pinned row itself sits right under the header.
        assert_eq!(track_offset(0, HDR, 0, 1, 1, sz), 20.0);
    }
}
