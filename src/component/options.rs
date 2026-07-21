#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Edit,
    /// Read-only mount for casual look-up on mobile. Disables
    /// the cell editor, the formula bar, copy/cut/paste, and
    /// the fill handle. Toolbar collapses to read-only actions
    /// (Print, Zoom, Sheet tabs). Phase 7 (mobile view-only).
    ViewOnly,
}

#[derive(Debug, Clone)]
pub struct Options {
    pub mode: Mode,
    pub show_grid: bool,
    pub show_toolbar: bool,
    pub show_bottom_bar: bool,
    pub show_context_menu: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            mode: Mode::Edit,
            show_grid: true,
            show_toolbar: true,
            show_bottom_bar: true,
            show_context_menu: true,
        }
    }
}
