use std::collections::HashMap;
use std::fmt::Display;
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cell {
    pub text: String,
    pub value: String,
    pub style: Option<usize>,
    pub merge: Option<(usize, usize)>, // (row_span, col_span)
    pub editable: bool,
    pub cell_type: String,
    /// An attached comment/note, if any.
    #[serde(default)]
    pub note: Option<String>,
}

impl Default for Cell {
    fn default() -> Self {
        Cell {
            text: String::new(),
            value: String::new(),
            style: None,
            merge: None,
            editable: true,
            cell_type: String::from("text"),
            note: None,
        }
    }
}

impl Cell {
    pub fn new() -> Self {
        Cell::default()
    }

    pub fn with_text(text: &str) -> Self {
        Cell {
            text: text.to_string(),
            value: text.to_string(),
            ..Default::default()
        }
    }

    pub fn set_text(&mut self, text: &str) {
        self.text = text.to_string();
    }

    pub fn set_value(&mut self, value: &str) {
        self.value = value.to_string();
    }

    pub fn set_style(&mut self, style_idx: usize) {
        self.style = Some(style_idx);
    }
}