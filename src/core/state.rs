use crate::core::cell_range::CellRange;

#[derive(Debug, Clone)]
pub struct Selector {
    pub range: CellRange,
    pub ri: usize,
    pub ci: usize,
}

impl Default for Selector {
    fn default() -> Self {
        Selector {
            range: CellRange::new(0, 0, 0, 0),
            ri: 0,
            ci: 0,
        }
    }
}

impl Selector {
    pub fn new() -> Self {
        Selector::default()
    }

    pub fn set_indexes(&mut self, ri: usize, ci: usize) {
        self.ri = ri;
        self.ci = ci;
    }

    pub fn set_range(&mut self, range: CellRange) {
        self.range = range;
    }

    pub fn multiple(&self) -> bool {
        self.range.multiple()
    }

    pub fn size(&self) -> (usize, usize) {
        self.range.size()
    }
}

#[derive(Debug, Clone)]
pub struct Scroll {
    pub ri: usize,
    pub ci: usize,
    pub x: f64,
    pub y: f64,
}

impl Default for Scroll {
    fn default() -> Self {
        Scroll {
            ri: 0,
            ci: 0,
            x: 0.0,
            y: 0.0,
        }
    }
}

impl Scroll {
    pub fn new() -> Self {
        Scroll::default()
    }

    pub fn set_x(&mut self, x: f64) {
        self.x = x;
    }

    pub fn set_y(&mut self, y: f64) {
        self.y = y;
    }

    pub fn set_indexes(&mut self, ri: usize, ci: usize) {
        self.ri = ri;
        self.ci = ci;
    }
}

#[derive(Debug, Clone)]
pub struct Clipboard {
    pub range: CellRange,
    pub state: String, // "copy" | "cut" | "clear"
}

impl Default for Clipboard {
    fn default() -> Self {
        Clipboard {
            range: CellRange::new(0, 0, 0, 0),
            state: String::from("clear"),
        }
    }
}

impl Clipboard {
    pub fn new() -> Self {
        Clipboard::default()
    }

    pub fn copy(&mut self, range: CellRange) {
        self.range = range;
        self.state = String::from("copy");
    }

    pub fn cut(&mut self, range: CellRange) {
        self.range = range;
        self.state = String::from("cut");
    }

    pub fn clear(&mut self) {
        self.state = String::from("clear");
    }

    pub fn is_copy(&self) -> bool {
        self.state == "copy"
    }

    pub fn is_cut(&self) -> bool {
        self.state == "cut"
    }

    pub fn is_clear(&self) -> bool {
        self.state == "clear"
    }
}

#[derive(Debug, Clone)]
pub struct History {
    undo_stack: Vec<String>,
    redo_stack: Vec<String>,
}

impl Default for History {
    fn default() -> Self {
        History {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }
}

impl History {
    pub fn new() -> Self {
        History::default()
    }

    pub fn add(&mut self, data: String) {
        self.undo_stack.push(data);
        self.redo_stack.clear();
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    pub fn undo<F>(&mut self, get_data: F, _set_data: F)
    where
        F: Fn() -> String,
    {
        if let Some(_state) = self.undo_stack.pop() {
            self.redo_stack.push(get_data());
        }
    }

    pub fn redo<F>(&mut self, get_data: F, _set_data: F)
    where
        F: Fn() -> String,
    {
        if let Some(_state) = self.redo_stack.pop() {
            self.undo_stack.push(get_data());
        }
    }
}
