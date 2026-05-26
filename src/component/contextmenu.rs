use crate::component::element::{Element, h};

#[derive(Debug, Clone)]
pub struct ResizerState {
    pub resizing_row: bool,
    pub resizing_col: bool,
    pub start_x: f64,
    pub start_y: f64,
    pub start_width: f64,
    pub start_height: f64,
    pub row_index: usize,
    pub col_index: usize,
}

impl Default for ResizerState {
    fn default() -> Self {
        ResizerState {
            resizing_row: false,
            resizing_col: false,
            start_x: 0f64,
            start_y: 0f64,
            start_width: 0f64,
            start_height: 0f64,
            row_index: 0,
            col_index: 0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Resizer {
    state: ResizerState,
    element: Element,
}

impl Resizer {
    pub fn new() -> Self {
        let element = h("div", Some("x-resizer"));
        Resizer {
            state: ResizerState::default(),
            element,
        }
    }

    pub fn start_row_resize(&mut self, row_index: usize, y: f64, height: f64) {
        self.state.resizing_row = true;
        self.state.row_index = row_index;
        self.state.start_y = y;
        self.state.start_height = height;
    }

    pub fn start_col_resize(&mut self, col_index: usize, x: f64, width: f64) {
        self.state.resizing_col = true;
        self.state.col_index = col_index;
        self.state.start_x = x;
        self.state.start_width = width;
    }

    pub fn end_resize(&mut self) {
        self.state.resizing_row = false;
        self.state.resizing_col = false;
    }

    pub fn is_resizing(&self) -> bool {
        self.state.resizing_row || self.state.resizing_col
    }

    pub fn handle_move(&mut self, x: f64, y: f64) -> Option<(usize, f64)> {
        if self.state.resizing_row {
            let delta = y - self.state.start_y;
            let new_height = self.state.start_height + delta;
            if new_height > 10f64 {
                return Some((self.state.row_index, new_height));
            }
        }
        if self.state.resizing_col {
            let delta = x - self.state.start_x;
            let new_width = self.state.start_width + delta;
            if new_width > 20f64 {
                return Some((self.state.col_index, new_width));
            }
        }
        None
    }

    pub fn element(&self) -> &Element {
        &self.element
    }
}

#[derive(Debug, Clone)]
pub struct ContextMenuState {
    pub mode: String,
    pub items: Vec<ContextMenuItem>,
    pub visible: bool,
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone)]
pub struct ContextMenuItem {
    pub label: String,
    pub action: String,
    pub disabled: bool,
    pub divider: bool,
}

impl Default for ContextMenuState {
    fn default() -> Self {
        ContextMenuState {
            mode: "range".to_string(),
            items: vec![],
            visible: false,
            x: 0f64,
            y: 0f64,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ContextMenu {
    state: ContextMenuState,
    element: Element,
}

impl ContextMenu {
    pub fn new() -> Self {
        let mut element = h("div", Some("x-contextmenu"));
        element.set_visibility(false);

        let mut menu = ContextMenu {
            state: ContextMenuState::default(),
            element,
        };
        menu.build_menu_items();
        menu
    }

    fn build_menu_items(&mut self) {
        self.state.items = vec![
            ContextMenuItem { label: "Copy".to_string(), action: "copy".to_string(), disabled: false, divider: false },
            ContextMenuItem { label: "Cut".to_string(), action: "cut".to_string(), disabled: false, divider: false },
            ContextMenuItem { label: "Paste".to_string(), action: "paste".to_string(), disabled: false, divider: false },
            ContextMenuItem { label: "Paste Values".to_string(), action: "paste-values".to_string(), disabled: false, divider: false },
            ContextMenuItem { label: "Paste Format".to_string(), action: "paste-format".to_string(), disabled: false, divider: true },
            ContextMenuItem { label: "Insert Row".to_string(), action: "insert-row".to_string(), disabled: false, divider: false },
            ContextMenuItem { label: "Insert Column".to_string(), action: "insert-col".to_string(), disabled: false, divider: false },
            ContextMenuItem { label: "Delete Row".to_string(), action: "delete-row".to_string(), disabled: false, divider: false },
            ContextMenuItem { label: "Delete Column".to_string(), action: "delete-col".to_string(), disabled: false, divider: false },
            ContextMenuItem { label: "Delete Cell Text".to_string(), action: "delete-cell".to_string(), disabled: false, divider: true },
            ContextMenuItem { label: "Hide".to_string(), action: "hide".to_string(), disabled: false, divider: false },
            ContextMenuItem { label: "Data Validation".to_string(), action: "validation".to_string(), disabled: false, divider: false },
            ContextMenuItem { label: "Cell Printable".to_string(), action: "printable".to_string(), disabled: false, divider: false },
            ContextMenuItem { label: "Cell Editable".to_string(), action: "editable".to_string(), disabled: false, divider: false },
        ];
    }

    pub fn set_mode(&mut self, mode: &str) {
        self.state.mode = mode.to_string();
    }

    pub fn show(&mut self, x: f64, y: f64) {
        self.state.x = x;
        self.state.y = y;
        self.state.visible = true;
        self.element.set_visibility(true);
    }

    pub fn hide(&mut self) {
        self.state.visible = false;
        self.element.set_visibility(false);
    }

    pub fn is_visible(&self) -> bool {
        self.state.visible
    }

    pub fn position(&self) -> (f64, f64) {
        (self.state.x, self.state.y)
    }

    pub fn items(&self) -> &[ContextMenuItem] {
        &self.state.items
    }

    pub fn element(&self) -> &Element {
        &self.element
    }
}

#[derive(Debug, Clone)]
pub struct SortFilterState {
    pub visible: bool,
    pub column_index: usize,
    pub sort_order: Option<String>,
    pub filters: Vec<FilterItem>,
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone)]
pub struct FilterItem {
    pub value: String,
    pub checked: bool,
}

impl Default for SortFilterState {
    fn default() -> Self {
        SortFilterState {
            visible: false,
            column_index: 0,
            sort_order: None,
            filters: vec![],
            x: 0f64,
            y: 0f64,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SortFilter {
    state: SortFilterState,
    element: Element,
}

impl SortFilter {
    pub fn new() -> Self {
        let mut element = h("div", Some("x-sort-filter"));
        element.set_visibility(false);

        SortFilter {
            state: SortFilterState::default(),
            element,
        }
    }

    pub fn show(&mut self, column_index: usize, x: f64, y: f64, items: Vec<String>) {
        self.state.column_index = column_index;
        self.state.x = x;
        self.state.y = y;
        self.state.visible = true;
        self.state.filters = items.into_iter().map(|v| FilterItem { value: v, checked: true }).collect();
        self.element.set_visibility(true);
    }

    pub fn hide(&mut self) {
        self.state.visible = false;
        self.element.set_visibility(false);
    }

    pub fn is_visible(&self) -> bool {
        self.state.visible
    }

    pub fn sort_asc(&mut self) {
        self.state.sort_order = Some("asc".to_string());
        self.hide();
    }

    pub fn sort_desc(&mut self) {
        self.state.sort_order = Some("desc".to_string());
        self.hide();
    }

    pub fn clear_sort(&mut self) {
        self.state.sort_order = None;
    }

    pub fn apply_filter(&mut self) {
        self.hide();
    }

    pub fn element(&self) -> &Element {
        &self.element
    }
}