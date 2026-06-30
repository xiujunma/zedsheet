use crate::component::element::{h, Element};

#[derive(Debug, Clone, Default)]
pub struct SelectorState {
    pub ri: usize,      // row index
    pub ci: usize,      // col index
    pub eri: usize,     // end row index
    pub eci: usize,     // end col index
    pub move_ri: usize, // move start row
    pub move_ci: usize, // move start col
    pub mouse_down: bool,
    pub dragging: bool,
}

#[derive(Debug, Clone)]
pub struct Selector {
    state: SelectorState,
    element: Element,
}

impl Selector {
    pub fn new() -> Self {
        let element = h("div", Some("x-selector"));
        Selector {
            state: SelectorState::default(),
            element,
        }
    }

    pub fn set(&mut self, ri: usize, ci: usize) {
        self.state.ri = ri;
        self.state.ci = ci;
        self.state.eri = ri;
        self.state.eci = ci;
        self.state.move_ri = ri;
        self.state.move_ci = ci;
    }

    pub fn set_range(&mut self, sri: usize, sci: usize, eri: usize, eci: usize) {
        self.state.ri = sri;
        self.state.ci = sci;
        self.state.eri = eri;
        self.state.eci = eci;
    }

    pub fn set_end(&mut self, ri: usize, ci: usize) {
        self.state.eri = ri;
        self.state.eci = ci;
    }

    pub fn indexes(&self) -> (usize, usize) {
        (self.state.ri, self.state.ci)
    }

    pub fn range(&self) -> (usize, usize, usize, usize) {
        (self.state.ri, self.state.ci, self.state.eri, self.state.eci)
    }

    pub fn is_single_selected(&self) -> bool {
        self.state.ri == self.state.eri && self.state.ci == self.state.eci
    }

    pub fn contains(&self, ri: usize, ci: usize) -> bool {
        let min_r = self.state.ri.min(self.state.eri);
        let max_r = self.state.ri.max(self.state.eri);
        let min_c = self.state.ci.min(self.state.eci);
        let max_c = self.state.ci.max(self.state.eci);
        ri >= min_r && ri <= max_r && ci >= min_c && ci <= max_c
    }

    pub fn element(&self) -> &Element {
        &self.element
    }
}

#[derive(Debug, Clone, Default)]
pub struct EditorState {
    pub active: bool,
    pub editing: bool,
    pub text: String,
    pub ri: usize,
    pub ci: usize,
    pub formula: bool,
}

#[derive(Debug, Clone)]
pub struct Editor {
    state: EditorState,
    element: Element,
    textarea: Option<Element>,
    container: Option<Element>,
}

impl Editor {
    pub fn new() -> Self {
        let mut element = h("div", Some("x-editor"));
        element.set_visibility(false);

        Editor {
            state: EditorState::default(),
            element,
            textarea: None,
            container: None,
        }
    }

    pub fn attach_to_container(&mut self, container: Element) {
        self.container = Some(container);
    }

    pub fn start_edit(
        &mut self,
        ri: usize,
        ci: usize,
        text: &str,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    ) {
        self.state.ri = ri;
        self.state.ci = ci;
        self.state.text = text.to_string();
        self.state.editing = true;
        self.state.active = true;
        self.state.formula = text.starts_with('=');

        // Create textarea if not exists
        if self.textarea.is_none() {
            let mut ta = h("textarea", Some("x-editor-textarea"));
            ta.set_position_absolute();
            ta.set_z_index(1000);
            ta.set_visibility(false);
            self.textarea = Some(ta);
        }

        if let Some(ref mut ta) = self.textarea {
            ta.set_visibility(true);
            ta.set_textarea_value(text);
            ta.set_top(y);
            ta.set_left(x);
            ta.set_width(width);
            ta.set_height(height);
            ta.focus();

            // Append to container if exists
            if let Some(ref mut container) = self.container {
                container.append_child(ta);
            }
        }
    }

    pub fn done_edit(&mut self) -> String {
        self.state.editing = false;
        self.state.active = false;

        let text = self.state.text.clone();

        if let Some(ref mut ta) = self.textarea {
            ta.set_visibility(false);
        }

        text
    }

    pub fn is_editing(&self) -> bool {
        self.state.editing
    }

    pub fn set_text(&mut self, text: &str) {
        self.state.text = text.to_string();
        if let Some(ref mut ta) = self.textarea {
            ta.set_textarea_value(text);
        }
    }

    pub fn element(&self) -> &Element {
        &self.element
    }

    pub fn is_active(&self) -> bool {
        self.state.active
    }
}

#[derive(Debug, Clone)]
pub struct ScrollbarState {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub scroll_width: f64,
    pub scroll_height: f64,
    pub thumb_x: f64,
    pub thumb_y: f64,
    pub thumb_width: f64,
    pub thumb_height: f64,
    pub dragging: bool,
}

impl Default for ScrollbarState {
    fn default() -> Self {
        ScrollbarState {
            x: 0f64,
            y: 0f64,
            width: 16f64,
            height: 16f64,
            scroll_width: 0f64,
            scroll_height: 0f64,
            thumb_x: 0f64,
            thumb_y: 0f64,
            thumb_width: 16f64,
            thumb_height: 16f64,
            dragging: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Scrollbar {
    state: ScrollbarState,
    element: Element,
    vertical: bool,
}

impl Scrollbar {
    pub fn new(vertical: bool) -> Self {
        let class = if vertical {
            "x-scrollbar-v"
        } else {
            "x-scrollbar-h"
        };
        let element = h("div", Some(class));

        Scrollbar {
            state: ScrollbarState::default(),
            element,
            vertical,
        }
    }

    pub fn set_params(
        &mut self,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        content_width: f64,
        content_height: f64,
    ) {
        self.state.x = x;
        self.state.y = y;
        self.state.width = width;
        self.state.height = height;
        self.state.scroll_width = content_width;
        self.state.scroll_height = content_height;

        if self.vertical {
            let ratio = height / content_height;
            self.state.thumb_height = (height * ratio).max(20f64).min(height);
            self.state.thumb_width = width;
        } else {
            let ratio = width / content_width;
            self.state.thumb_width = (width * ratio).max(20f64).min(width);
            self.state.thumb_height = height;
        }
    }

    pub fn move_to(&mut self, x: f64, y: f64) {
        if self.vertical {
            self.state.thumb_y = y;
        } else {
            self.state.thumb_x = x;
        }
    }

    pub fn element(&self) -> &Element {
        &self.element
    }

    pub fn scroll(&self, delta_x: f64, delta_y: f64) -> (f64, f64) {
        (delta_x, delta_y)
    }
}
