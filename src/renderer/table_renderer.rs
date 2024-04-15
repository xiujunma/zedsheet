#![allow(dead_code)]

use std::fmt::Display;

use crate::renderer::alphabets::exp2xy;
use crate::renderer::canvas::Canvas;
use crate::renderer::render::{AreaRenderer, render};
use web_sys::HtmlCanvasElement;

use super::alphabets::string_at;
use super::viewport::Viewport;
use crate::data::table_data::TableData;

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
    value: String,
    cell_type: String,
    style: usize,
    formula: String,
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
    pub data: TableData,
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
}

impl TableRenderer {
    pub fn new(container: HtmlCanvasElement, width: f64, height: f64, data: TableData) -> TableRenderer {
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
        }
    }

    pub fn render(&mut self) {
        self.viewport = Some(Viewport::new(
            self.data.clone(),
            self.freeze,
            self.start_row,
            self.start_col,
            self.rows,
            self.cols,
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
        let r = self.data.get_row(index);
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
        let c: Option<Col> = self.data.get_col(index);
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
}
