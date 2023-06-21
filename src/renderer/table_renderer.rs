#![allow(dead_code)]


use web_sys::HtmlCanvasElement;

use super::viewport::Viewport;

pub enum Align {
    Left,
    Right,
    Center,
}

pub enum VerticalAlign {
    Top,
    Middle,
    Bottom,
}

pub enum GridlineStyle {
    Solid,
    Dashed,
    Dotted,
}

pub struct Gridline {
    width: usize,
    color: String,
    style: Option<GridlineStyle>,
}

pub enum TextLineType {
    Underline,
    StrikeThrough,
}

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

pub enum BorderLineStyle {
    Thin,
    Medium,
    Thick,
    Dashed,
    Dotted,
}

pub struct BorderLine {
    left: Option<(BorderLineStyle, String)>,
    top: Option<(BorderLineStyle, String)>,
    right: Option<(BorderLineStyle, String)>,
    bottom: Option<(BorderLineStyle, String)>,
}

pub struct Border {
    reference: String,
    border_type: BorderType,
    border_line: BorderLineStyle,
    color: String
}

pub struct Style {
    bgcolor: Option<String>,
    color: String,
    align: Align,
    valign: VerticalAlign,
    textwrap: bool,
    underline: bool,
    strike_through: bool,
    bold: bool,
    italic: bool,
    font_size: usize,
    font_family: String,
    rotation: Option<usize>,
    padding: Option<(usize, usize)>
}

pub struct Cell {
    value: String,
    cell_type: String,
    style: usize,
    formula: String
}

pub struct Row {
    height: f64,
    hide: bool,
    auto_fit: bool,
    style: usize
}

pub struct Col {
    width: f64,
    hide: bool,
    auto_fit: bool,
    style: usize
}

pub struct RowHeader {
    height: f64,
    cols: usize
}

pub struct ColHeader {
    width: f64,
    rows: usize
}

pub struct Rect {
    x: f64,
    y: f64,
    width: f64,
    height: f64
}

pub struct ViewportCell {

}

pub struct TableRenderer {
    target: HtmlCanvasElement,
    bgcolor: String,
    width: f64,
    height: f64,
    scale: f64,
    rows: f64,
    cols: f64,
    row_height: f64,
    col_width: f64,
    start_row: f64,
    start_col: f64,
    scroll_rows: f64,
    scroll_cols: f64,
    merges: Vec<String>,
    borders: Vec<Border>,
    styles: Vec<Style>,
    gridline: Gridline,
    style: Style, // default style
    col_header: ColHeader,
    row_header: RowHeader,
    header_gridline: Gridline,
    header_style: Style,
    freeze: (usize, usize),
    freeze_gridline: Gridline,
    // viewport: Viewport
}