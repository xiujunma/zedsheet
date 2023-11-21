#![allow(dead_code)]

use crate::renderer::alphabets::exp2xy;
use crate::renderer::canvas::Canvas;
use crate::renderer::render::render;
use web_sys::HtmlCanvasElement;

use super::viewport::Viewport;

type RowGetter = fn(usize) -> Option<Row>;

type ColGetter = fn(usize) -> Option<Col>;

type CellGetter = fn(usize, usize) -> Option<Cell>;

pub type CellRenderer = fn(Canvas, Rect, Cell, String) -> bool;

pub type Formatter = fn(Cell, String) -> String;

#[derive(PartialEq)]
pub enum Align {
    Left,
    Right,
    Center,
}
#[derive(PartialEq)]
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
    pub width: f64,
    pub color: String,
    pub style: Option<GridlineStyle>,
}

#[derive(PartialEq)]
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

#[derive(PartialEq, Copy, Clone)]
pub enum BorderLineStyle {
    Thin,
    Medium,
    Thick,
    Dashed,
    Dotted,
}

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

pub struct Border {
    reference: String,
    border_type: BorderType,
    border_line: BorderLineStyle,
    color: String,
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
#[derive(Debug, Clone)]
pub struct Cell {
    value: String,
    cell_type: String,
    style: usize,
    formula: String,
}
#[derive(Debug, Clone)]
pub struct Row {
    height: f64,
    hide: bool,
    auto_fit: bool,
    style: usize,
}
#[derive(Debug, Clone)]
pub struct Col {
    width: f64,
    hide: bool,
    auto_fit: bool,
    style: usize,
}
#[derive(Debug, Clone)]
pub struct RowHeader {
    pub width: f64,
    pub cols: usize,
    pub cell: CellGetter,
    pub cell_renderer: CellRenderer,
    pub merges: Vec<String>,
}
#[derive(Debug, Clone)]
pub struct ColHeader {
    pub height: f64,
    pub rows: usize,
    pub cell: CellGetter,
    pub cell_renderer: CellRenderer,
    pub merges: Vec<String>,
}
#[derive(Debug, Clone)]
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
#[derive(Debug, Clone)]
pub struct TableRenderer<'a> {
    pub target: HtmlCanvasElement,
    pub bgcolor: String,
    pub width: f64,
    pub height: f64,
    pub scale: f64,
    pub rows: f64,
    pub cols: f64,
    pub row_height: f64,
    pub col_width: f64,
    pub start_row: usize,
    pub start_col: usize,
    pub scroll_rows: f64,
    pub scroll_cols: f64,
    pub row: RowGetter,
    pub col: ColGetter,
    pub cell: CellGetter,
    pub cell_renderer: CellRenderer,
    pub formatter: Formatter,
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
    pub viewport: &'a Viewport<'a>,
}

impl<'a> TableRenderer<'a> {
    pub fn new(container: HtmlCanvasElement, width: f64, height: f64) -> TableRenderer<'a> {
        TableRenderer {
            target: container,
            width,
            height,
            ..Default::default()
        }
    }

    fn render(&mut self) -> &Self {
        let viewport = Viewport::new(&self);
        self.viewport = &viewport;
        render(self);
        return self;
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

    fn rows(&mut self, rows: f64) -> &Self {
        self.rows = rows;
        return self;
    }

    fn cols(&mut self, cols: f64) -> &Self {
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

    fn scroll_rows(&mut self, scroll_rows: f64) -> &Self {
        self.scroll_rows = scroll_rows;
        return self;
    }

    fn scroll_cols(&mut self, scroll_cols: f64) -> &Self {
        self.scroll_cols = scroll_cols;
        return self;
    }

    fn row(&mut self, row: RowGetter) -> &Self {
        self.row = row;
        return self;
    }

    fn col(&mut self, col: ColGetter) -> &Self {
        self.col = col;
        return self;
    }

    fn cell(&mut self, cell: CellGetter) -> &Self {
        self.cell = cell;
        return self;
    }

    fn cell_renderer(&mut self, cell_renderer: CellRenderer) -> &Self {
        self.cell_renderer = cell_renderer;
        return self;
    }

    fn formatter(&mut self, formatter: Formatter) -> &Self {
        self.formatter = formatter;
        return self;
    }

    fn merges(&mut self, merges: Vec<String>) -> &Self {
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

    fn freeze(&mut self, reference: &str) -> &Self {
        let (x, y) = exp2xy(reference);
        self.freeze = (y, x);
        return self;
    }

    fn freeze_gridline(&mut self, freeze_gridline: Gridline) -> &Self {
        self.freeze_gridline = freeze_gridline;
        return self;
    }

    pub fn row_height_at(&self, index: usize) -> f64 {
        let r = self.row.get_row(index);
        return match r {
            Some(row) => {
                if row.hide {
                    0f64
                } else {
                    row.height
                }
            }
            None => self.row_height,
        };
    }

    pub fn col_width_at(&self, index: usize) -> f64 {
        let c = self.col.get_col(index);
        return match c {
            Some(col) => {
                if col.hide {
                    0f64
                } else {
                    col.width
                }
            }
            None => self.col_width,
        };
    }

    fn get_viewport(&self) -> &Viewport {
        &self.viewport
    }
}
