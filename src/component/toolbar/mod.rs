use crate::component::element::{Element, h};
use crate::config::CSS_PREFIX;

#[derive(Debug, Clone)]
pub struct ToolbarState {
    pub undo_enabled: bool,
    pub redo_enabled: bool,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub text_wrap: bool,
    pub merged: bool,
    pub frozen: bool,
    pub autofilter_active: bool,
    pub align: String,
    pub valign: String,
    pub font: String,
    pub font_size: usize,
    pub text_color: String,
    pub fill_color: String,
}

impl Default for ToolbarState {
    fn default() -> Self {
        ToolbarState {
            undo_enabled: true,
            redo_enabled: true,
            bold: false,
            italic: false,
            underline: false,
            strike: false,
            text_wrap: false,
            merged: false,
            frozen: false,
            autofilter_active: false,
            align: "left".to_string(),
            valign: "middle".to_string(),
            font: "Arial".to_string(),
            font_size: 10,
            text_color: "#000000".to_string(),
            fill_color: "#ffffff".to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Toolbar {
    state: ToolbarState,
    element: Element,
    buttons: Vec<Element>,
}

impl Toolbar {
    pub fn new() -> Self {
        let element = h("div", Some(&format!("{}-toolbar", CSS_PREFIX)));
        let buttons = vec![];

        let mut toolbar = Toolbar {
            state: ToolbarState::default(),
            element,
            buttons,
        };
        toolbar.build_toolbar();
        toolbar
    }

    fn build_toolbar(&mut self) {
        // Each entry is (data-action, sprite-icon-class). `None` marks a group
        // divider. Icon classes match the sprite positions in index.css.
        let items: Vec<Option<(&str, &str)>> = vec![
            Some(("undo", "undo")),
            Some(("redo", "redo")),
            None,
            Some(("print", "print")),
            Some(("paintformat", "paintformat")),
            Some(("clearformat", "clearformat")),
            None,
            Some(("font-bold", "font-bold")),
            Some(("font-italic", "font-italic")),
            Some(("underline", "underline")),
            Some(("strike", "strike")),
            Some(("color", "color")),
            Some(("bgcolor", "bgcolor")),
            None,
            Some(("merge", "merge")),
            Some(("borders", "border-all")),
            None,
            Some(("align-left", "align-left")),
            Some(("align-center", "align-center")),
            Some(("align-right", "align-right")),
            // Vertical align is a dropdown (top/middle/bottom), matching
            // x-spreadsheet; the menu is registered in `zedsheet`.
            Some(("valign", "align-middle")),
            Some(("textwrap", "textwrap")),
            None,
            Some(("freeze", "freeze")),
            Some(("autofilter", "autofilter")),
            Some(("formula", "formula")),
        ];

        let mut btns = h("div", Some(&format!("{}-toolbar-btns", CSS_PREFIX)));

        // Dropdown buttons (format / font / fontsize) precede the icon groups.
        for (action, title, width, id) in [
            ("dd-format", "Normal", 72, "zs-dd-format"),
            ("dd-font", "Arial", 72, "zs-dd-font"),
            ("dd-fontsize", "10", 30, "zs-dd-fontsize"),
        ] {
            let mut btn = h("div", Some(&format!("{}-toolbar-btn", CSS_PREFIX)));
            btn.dataset_set("action", action);
            btn.dataset_set("tip", tip_for(action));
            btn.set_inner_html(format!(
                "<div class=\"{p}-dropdown bottom-left\"><div class=\"{p}-dropdown-header\">\
                   <div class=\"{p}-dropdown-title\" id=\"{id}\" style=\"display:inline-block;width:{w}px;text-align:left;padding:0 4px;line-height:26px;\">{title}</div>\
                   <div class=\"{p}-icon arrow-right\"><div class=\"{p}-icon-img arrow-down\"></div></div>\
                 </div></div>",
                p = CSS_PREFIX, id = id, w = width, title = title
            ));
            btns.append_child(&mut btn);
            self.buttons.push(btn);
        }
        let mut divider0 = h("div", Some(&format!("{}-toolbar-divider", CSS_PREFIX)));
        btns.append_child(&mut divider0);

        for item in items.iter() {
            match item {
                Some((action, icon)) => {
                    let mut btn = h("div", Some(&format!("{}-toolbar-btn", CSS_PREFIX)));
                    btn.dataset_set("action", action);
                    btn.dataset_set("tip", tip_for(action));
                    btn.set_inner_html(format!(
                        "<div class=\"{prefix}-icon\"><div class=\"{prefix}-icon-img {icon}\"></div></div>",
                        prefix = CSS_PREFIX,
                        icon = icon
                    ));
                    btns.append_child(&mut btn);
                    self.buttons.push(btn);
                }
                None => {
                    let mut divider = h("div", Some(&format!("{}-toolbar-divider", CSS_PREFIX)));
                    btns.append_child(&mut divider);
                }
            }
        }

        self.element.append_child(&mut btns);
    }

    pub fn set_state(&mut self, state: ToolbarState) {
        self.state = state;
        self.update_button_states();
    }

    pub fn state(&self) -> &ToolbarState {
        &self.state
    }

    fn update_button_states(&self) {
        // Update button visual states based on self.state
    }

    pub fn reset(&mut self) {
        // Reset toolbar state based on current selection
    }

    pub fn element(&self) -> &Element {
        &self.element
    }
}

impl Default for Toolbar {
    fn default() -> Self {
        Self::new()
    }
}

/// Human-readable tooltip text for a toolbar button's `data-action`.
fn tip_for(action: &str) -> &'static str {
    match action {
        "dd-format" => "Format",
        "dd-font" => "Font",
        "dd-fontsize" => "Font size",
        "undo" => "Undo",
        "redo" => "Redo",
        "print" => "Print",
        "paintformat" => "Paint format",
        "clearformat" => "Clear format",
        "font-bold" => "Bold (Ctrl+B)",
        "font-italic" => "Italic (Ctrl+I)",
        "underline" => "Underline (Ctrl+U)",
        "strike" => "Strikethrough",
        "color" => "Text color",
        "bgcolor" => "Fill color",
        "merge" => "Merge cells",
        "borders" => "Borders",
        "align-left" => "Align left",
        "align-center" => "Align center",
        "align-right" => "Align right",
        "valign" => "Vertical align",
        "textwrap" => "Text wrap",
        "freeze" => "Freeze",
        "autofilter" => "Filter",
        "formula" => "Functions",
        _ => "",
    }
}

pub mod items;