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
    color: String,
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
    padding: Option<(usize, usize)>,
}

pub struct Cell {
    value: String,
    cell_type: String,
    style: usize,
    formula: String,
}

pub struct Row {
    height: f64,
    hide: bool,
    auto_fit: bool,
    style: usize,
}

pub struct Col {
    width: f64,
    hide: bool,
    auto_fit: bool,
    style: usize,
}

pub struct RowHeader {
    pub height: f64,
    pub cols: usize,
}

pub struct ColHeader {
    pub width: f64,
    pub rows: usize,
}

pub struct Rect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

pub struct ViewportCell {}

pub trait GetRowHeightColWidth {
    fn get_row_height(&self, index: usize) -> f64;
    fn get_col_width(&self, index: usize) -> f64;
}

pub struct TableRenderer {
    pub target: HtmlCanvasElement,
    pub bgcolor: String,
    pub width: f64,
    pub height: f64,
    pub scale: f64,
    pub rows: f64,
    pub cols: f64,
    pub row_height: f64,
    pub col_width: f64,
    pub start_row: f64,
    pub start_col: f64,
    pub scroll_rows: f64,
    pub scroll_cols: f64,
    pub merges: Vec<String>,
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
    pub viewport: Viewport,
}

impl GetRowHeightColWidth for TableRenderer {
    fn get_row_height(&self, index: usize) -> f64 {
        return 0f64;
    }

    fn get_col_width(&self, index: usize) -> f64 {
        return 0f64;
    }
}

impl TableRenderer {
    pub fn new(container: HtmlCanvasElement, width: f64, height: f64) -> TableRenderer {
        TableRenderer { target: container, width, height, ..Default::default() }
    }
}

