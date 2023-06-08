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

struct TableRenderer {
    target: HtmlCanvasElement,
}