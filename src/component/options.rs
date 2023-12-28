#[derive(Debug, Clone)]
pub enum Mode {
    Normal,
    Edit
}

#[derive(Debug, Clone)]
pub struct Options {
    pub mode: Mode,
    pub show_grid: bool,
    pub show_toolbar: bool,
    pub show_bottom_bar: bool
}

impl Default for Options {
    fn default() -> Self {
        Self {
            mode: Mode::Edit,
            show_grid: true,
            show_toolbar: true,
            show_bottom_bar: true
        }
    }
}