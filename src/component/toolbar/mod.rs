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
        // Build toolbar structure with all buttons
        let items = vec![
            ("undo", "Undo"),
            ("redo", "Redo"),
            ("print", "Print"),
            ("paint-format", "Paint Format"),
            ("clear-format", "Clear Format"),
            ("bold", "B"),
            ("italic", "I"),
            ("underline", "U"),
            ("strike", "S"),
            ("merge", "Merge"),
            ("text-wrap", "Wrap"),
            ("freeze", "Freeze"),
            ("autofilter", "Filter"),
            ("formula", "fx"),
        ];

        // Build button elements
        for (id, label) in items.iter() {
            let mut btn = h("button", Some(&format!("{}-btn", CSS_PREFIX)));
            btn.dataset_set("action", id);
            btn.set_inner_html(label.to_string());
            self.element.append_child(&mut btn);
            self.buttons.push(btn);
        }
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

pub mod items;