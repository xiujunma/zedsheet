#![allow(dead_code)]

use web_sys::HtmlCanvasElement;
use crate::renderer::alphabets::exp2xy;
use crate::renderer::canvas::Canvas;
use crate::renderer::render::render;

use super::viewport::Viewport;

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
    width: usize,
    color: String,
    style: Option<GridlineStyle>,
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
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

pub struct ViewportCell {}

pub trait GetRowHeightColWidth {
    fn get_row_height(&self, index: usize) -> f64;
    fn get_col_width(&self, index: usize) -> f64;
}

pub trait RowGetter {
    fn get_row(&self, index: usize) -> Option<Row>;
}

pub trait ColGetter {
    fn get_col(&self, index: usize) -> Option<Col>;
}

pub trait CellGetter {
    fn get_cell(&self, row: usize, col: usize) -> Option<Cell>;
}

pub trait CellRenderer {
    fn render(&self, canvas: Canvas, rect: Rect, cell: Cell, text: String) -> bool;
}

pub trait Formatter {
    fn format(&self, cell: Cell) -> String;
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
    pub row: dyn RowGetter,
    pub col: dyn ColGetter,
    pub cell: dyn CellGetter,
    pub cell_renderer: dyn CellRenderer,
    pub formatter: dyn Formatter,
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

    fn render(&mut self) -> &self {
        let viewport = Viewport::new(&self);
        self.viewport = viewport;
        render(self);
        return self;
    }

    fn bgcolor(&mut self, color: String) -> &self {
        self.bgcolor = color;
        return self;
    }

    fn width(&mut self, width: f64) -> &self {
        self.width = width;
        return self;
    }

    fn height(&mut self, height: f64) -> &self {
        self.height = height;
        return self;
    }

    fn scale(&mut self, scale: f64) -> &self {
        self.scale = scale;
        return self;
    }

    fn rows(&mut self, rows: f64) -> &self {
        self.rows = rows;
        return self;
    }

    fn cols(&mut self, cols: f64) -> &self {
        self.cols = cols;
        return self;
    }

    fn row_height(&mut self, row_height: f64) -> &self {
        self.row_height = row_height;
        return self;
    }

    fn col_width(&mut self, col_width: f64) -> &self {
        self.col_width = col_width;
        return self;
    }

    fn start_row(&mut self, start_row: f64) -> &self {
        self.start_row = start_row;
        return self;
    }

    fn start_col(&mut self, start_col: f64) -> &self {
        self.start_col = start_col;
        return self;
    }

    fn scroll_rows(&mut self, scroll_rows: f64) -> &self {
        self.scroll_rows = scroll_rows;
        return self;
    }

    fn scroll_cols(&mut self, scroll_cols: f64) -> &self {
        self.scroll_cols = scroll_cols;
        return self;
    }

    fn row(&mut self, row: impl RowGetter) -> &self {
        self.row = row;
        return self;
    }

    fn col(&mut self, col: impl ColGetter) -> &self {
        self.col = col;
        return self;
    }

    fn cell(&mut self, cell: impl CellGetter) -> &self {
        self.cell = cell;
        return self;
    }

    fn cell_renderer(&mut self, cell_renderer: impl CellRenderer) -> &self {
        self.cell_renderer = cell_renderer;
        return self;
    }

    fn formatter(&mut self, formatter: impl Formatter) -> &self {
        self.formatter = formatter;
        return self;
    }

    fn merges(&mut self, merges: Vec<String>) -> &self {
        self.merges = merges;
        return self;
    }

    fn styles(&mut self, styles: Vec<Style>) -> &self {
        self.styles = styles;
        return self;
    }

    fn borders(&mut self, borders: Vec<Border>) -> &self {
        self.borders = borders;
        return self;
    }

    fn gridline(&mut self, gridline: Gridline) -> &self {
        self.gridline = gridline;
        return self;
    }

    fn style(&mut self, style: Style) -> &self {
        self.style = style;
        return self;
    }

    fn row_header(&mut self, row_header: RowHeader) -> &self {
        self.row_header = row_header;
        return self;
    }

    fn col_header(&mut self, col_header: ColHeader) -> &self {
        self.col_header = col_header;
        return self;
    }

    fn header_gridline(&mut self, header_gridline: Gridline) -> &self {
        self.header_gridline = header_gridline;
        return self;
    }

    fn header_style(&mut self, header_style: Style) -> &self {
        self.header_style = header_style;
        return self;
    }

    fn freeze(&mut self, reference: &str) -> &self {
        let (x, y) = exp2xy(reference);
        self.freeze = (y, x);
        return self;
    }

    fn freeze_gridline(&mut self, freeze_gridline: Gridline) -> &self {
        self.freeze_gridline = freeze_gridline;
        return self;
    }

    fn row_height_at(&self, index: usize) -> f64 {
        let r = self.row.get_row(index);
        return match r {
            Some(row) => {
                if row.hide { 0f64 } else { row.height }
            }
            None => {
                self.row_height
            }
        }
    }

    fn col_width_at(&self, index: usize) -> f64 {
        let c = self.col.get_col(index);
        return match c {
            Some(col) => {
                if col.hide { 0f64 } else { col.width }
            }
            None => {
                self.col_width
            }
        }
    }

    fn get_viewport(&self) -> &Viewport {
        &self.viewport
    }
}

