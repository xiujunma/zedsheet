use serde::{Serialize, Deserialize};
use std::collections::{HashMap, HashSet};
use crate::core::cell_range::CellRange;
use crate::formula::parser::{tokenize, Token};
use crate::renderer::alphabets::{exp2xy, index_at, string_at};
use regex::Regex;
use crate::core::cell::Cell;
use crate::core::row::Row;
use crate::core::col::{Col, Cols};
use crate::core::merges::Merges;
use crate::core::state::{Selector, Scroll, Clipboard, History};
use crate::core::validation::Validations;
use crate::core::auto_filter::AutoFilter;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Style {
    pub bgcolor: Option<String>,
    pub color: String,
    pub align: String,
    pub valign: String,
    pub text_wrap: bool,
    pub underline: bool,
    pub strike: bool,
    pub bold: bool,
    pub italic: bool,
    pub font_size: usize,
    pub font_family: String,
    pub format: String,
    pub border: Option<Border>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Border {
    pub left: Option<(String, String)>,
    pub right: Option<(String, String)>,
    pub top: Option<(String, String)>,
    pub bottom: Option<(String, String)>,
}

impl Default for Style {
    fn default() -> Self {
        Style {
            bgcolor: Some("#ffffff".to_string()),
            color: "#0a0a0a".to_string(),
            align: "left".to_string(),
            valign: "middle".to_string(),
            text_wrap: false,
            underline: false,
            strike: false,
            bold: false,
            italic: false,
            font_size: 10,
            font_family: "Arial".to_string(),
            format: "normal".to_string(),
            border: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DataProxy {
    pub name: String,
    pub freeze: (usize, usize),
    pub styles: Vec<Style>,
    pub merges: Merges,
    pub rows: HashMap<usize, Row>,
    pub row_count: usize,
    pub default_row_height: f64,
    pub cols: Cols,
    pub validations: Validations,
    pub selector: Selector,
    pub scroll: Scroll,
    pub history: History,
    pub clipboard: Clipboard,
    pub auto_filter: AutoFilter,
}

impl Default for DataProxy {
    fn default() -> Self {
        DataProxy {
            name: "sheet".to_string(),
            freeze: (0, 0),
            styles: Vec::new(),
            merges: Merges::new(),
            rows: HashMap::new(),
            row_count: 100,
            default_row_height: 25.0,
            cols: Cols::new(26, 100.0),
            validations: Validations::new(),
            selector: Selector::new(),
            scroll: Scroll::new(),
            history: History::new(),
            clipboard: Clipboard::new(),
            auto_filter: AutoFilter::new(),
        }
    }
}

impl DataProxy {
    pub fn new(name: &str) -> Self {
        let mut dp = DataProxy::default();
        dp.name = name.to_string();
        dp
    }

    pub fn get_cell(&self, ri: usize, ci: usize) -> Option<&Cell> {
        self.rows.get(&ri).and_then(|row| row.get_cell(ci))
    }

    pub fn get_row(&self, ri: usize) -> Option<&Row> {
        self.rows.get(&ri)
    }

    pub fn get_col(&self, ci: usize) -> Option<&Col> {
        self.cols.get(ci)
    }

    pub fn row_count(&self) -> usize {
        self.row_count
    }

    pub fn col_count(&self) -> usize {
        self.cols.len
    }

    pub fn get_cell_or_new(&mut self, ri: usize, ci: usize) -> &mut Cell {
        let row = self.rows.entry(ri).or_insert_with(Row::default);
        row.get_cell_or_new(ci)
    }

    pub fn get_cell_mut(&mut self, ri: usize, ci: usize) -> Option<&mut Cell> {
        self.rows.get_mut(&ri).and_then(|row| row.get_cell_mut(ci))
    }

    pub fn set_cell_text(&mut self, ri: usize, ci: usize, text: &str) {
        let cell = self.get_cell_or_new(ri, ci);
        cell.set_text(text);
    }

    pub fn get_cell_text(&self, ri: usize, ci: usize) -> String {
        self.get_cell(ri, ci).map(|c| c.text.clone()).unwrap_or_default()
    }

    /// The text shown for a cell: formulas (text starting with `=`) are
    /// evaluated; everything else is returned verbatim.
    pub fn cell_display_value(&self, ri: usize, ci: usize) -> String {
        let text = self.get_cell_text(ri, ci);
        // A cell that literally holds an error value displays it verbatim.
        if EvalErr::from_literal(&text).is_some() {
            return text;
        }
        let raw = if let Some(expr) = text.strip_prefix('=') {
            let mut visited = HashSet::new();
            visited.insert((ri, ci));
            match self.eval_expr(expr, &mut visited) {
                Ok(v) => format_number(v),
                Err(e) => return e.code().to_string(),
            }
        } else {
            text
        };
        // Apply the cell's display format (number/currency/percent/…).
        let fmt = self.get_cell_style(ri, ci).format;
        crate::core::format::format_value(&raw, &fmt)
    }

    /// Resolve a cell to a numeric value for use inside a formula. Error values
    /// propagate; non-numeric text resolves to 0; nested formulas recurse with
    /// a circular-ref guard.
    fn resolve_numeric(&self, ri: usize, ci: usize, visited: &mut HashSet<(usize, usize)>) -> Result<f64, EvalErr> {
        if visited.contains(&(ri, ci)) {
            return Ok(0.0);
        }
        let text = self.get_cell_text(ri, ci);
        if let Some(e) = EvalErr::from_literal(&text) {
            return Err(e);
        }
        if let Some(expr) = text.strip_prefix('=') {
            visited.insert((ri, ci));
            let v = self.eval_expr(expr, visited);
            visited.remove(&(ri, ci));
            v
        } else {
            Ok(text.trim().parse::<f64>().unwrap_or(0.0))
        }
    }

    fn eval_expr(&self, expr: &str, visited: &mut HashSet<(usize, usize)>) -> Result<f64, EvalErr> {
        let tokens = tokenize(expr);
        let mut pos = 0usize;
        let v = self.parse_cmp(&tokens, &mut pos, visited)?;
        Ok(v)
    }

    // expr := add ((= | == | > | < | >= | <=) add)*  — comparisons yield 1.0/0.0
    fn parse_cmp(&self, t: &[Token], pos: &mut usize, vis: &mut HashSet<(usize, usize)>) -> Result<f64, EvalErr> {
        let mut v = self.parse_add(t, pos, vis)?;
        while *pos < t.len() {
            if let Token::Operator(op) = &t[*pos] {
                if matches!(op.as_str(), "=" | "==" | ">" | "<" | ">=" | "<=") {
                    let op = op.clone();
                    *pos += 1;
                    let r = self.parse_add(t, pos, vis)?;
                    let b = match op.as_str() {
                        "=" | "==" => v == r,
                        ">" => v > r,
                        "<" => v < r,
                        ">=" => v >= r,
                        "<=" => v <= r,
                        _ => false,
                    };
                    v = if b { 1.0 } else { 0.0 };
                    continue;
                }
            }
            break;
        }
        Ok(v)
    }

    // expr := term (('+' | '-') term)*
    fn parse_add(&self, t: &[Token], pos: &mut usize, vis: &mut HashSet<(usize, usize)>) -> Result<f64, EvalErr> {
        let mut v = self.parse_mul(t, pos, vis)?;
        while *pos < t.len() {
            if let Token::Operator(op) = &t[*pos] {
                if op == "+" || op == "-" {
                    *pos += 1;
                    let r = self.parse_mul(t, pos, vis)?;
                    v = if op == "+" { v + r } else { v - r };
                    continue;
                }
            }
            break;
        }
        Ok(v)
    }

    // term := factor (('*' | '/') factor)*
    fn parse_mul(&self, t: &[Token], pos: &mut usize, vis: &mut HashSet<(usize, usize)>) -> Result<f64, EvalErr> {
        let mut v = self.parse_factor(t, pos, vis)?;
        while *pos < t.len() {
            if let Token::Operator(op) = &t[*pos] {
                if op == "*" || op == "/" {
                    *pos += 1;
                    let r = self.parse_factor(t, pos, vis)?;
                    if op == "*" {
                        v *= r;
                    } else {
                        if r == 0.0 {
                            return Err(EvalErr::Div0);
                        }
                        v /= r;
                    }
                    continue;
                }
            }
            break;
        }
        Ok(v)
    }

    // factor := Number | '-' factor | '(' expr ')' | Function '(' args ')' | CellRef
    fn parse_factor(&self, t: &[Token], pos: &mut usize, vis: &mut HashSet<(usize, usize)>) -> Result<f64, EvalErr> {
        let tok = t.get(*pos).ok_or(EvalErr::Value)?.clone();
        match tok {
            Token::Number(n) => {
                *pos += 1;
                Ok(n)
            }
            Token::Error(code) => {
                *pos += 1;
                Err(EvalErr::from_literal(&code).unwrap_or(EvalErr::Value))
            }
            Token::Operator(op) if op == "-" => {
                *pos += 1;
                Ok(-self.parse_factor(t, pos, vis)?)
            }
            Token::Operator(op) if op == "+" => {
                *pos += 1;
                self.parse_factor(t, pos, vis)
            }
            Token::LeftParen => {
                *pos += 1;
                let v = self.parse_cmp(t, pos, vis)?;
                if matches!(t.get(*pos), Some(Token::RightParen)) {
                    *pos += 1;
                }
                Ok(v)
            }
            Token::Function(name) => {
                *pos += 1; // function name
                if matches!(t.get(*pos), Some(Token::LeftParen)) {
                    *pos += 1;
                }
                let args = self.parse_args(t, pos, vis)?;
                apply_function(&name, &args)
            }
            Token::CellRef(r) => {
                *pos += 1;
                let (c, row) = exp2xy(&r);
                self.resolve_numeric(row, c, vis)
            }
            Token::String(s) => {
                *pos += 1;
                Ok(s.trim().parse::<f64>().unwrap_or(0.0))
            }
            _ => Err(EvalErr::Value),
        }
    }

    // Parse a comma-separated argument list (until RightParen), flattening
    // any `A1:B3` ranges into the individual cell values they cover.
    fn parse_args(&self, t: &[Token], pos: &mut usize, vis: &mut HashSet<(usize, usize)>) -> Result<Vec<f64>, EvalErr> {
        let mut args = Vec::new();
        if matches!(t.get(*pos), Some(Token::RightParen)) {
            *pos += 1;
            return Ok(args);
        }
        loop {
            // Range: CellRef ':' CellRef
            if let (Some(Token::CellRef(a)), Some(Token::Colon), Some(Token::CellRef(b))) =
                (t.get(*pos), t.get(*pos + 1), t.get(*pos + 2))
            {
                let (c0, r0) = exp2xy(a);
                let (c1, r1) = exp2xy(b);
                let (r0, r1) = (r0.min(r1), r0.max(r1));
                let (c0, c1) = (c0.min(c1), c0.max(c1));
                for r in r0..=r1 {
                    for c in c0..=c1 {
                        args.push(self.resolve_numeric(r, c, vis)?);
                    }
                }
                *pos += 3;
            } else {
                args.push(self.parse_cmp(t, pos, vis)?);
            }

            match t.get(*pos) {
                Some(Token::Comma) => {
                    *pos += 1;
                }
                Some(Token::RightParen) => {
                    *pos += 1;
                    break;
                }
                None => break,
                _ => break,
            }
        }
        Ok(args)
    }

    /// The resolved style for a cell (from the styles table), or the default.
    pub fn get_cell_style(&self, ri: usize, ci: usize) -> Style {
        if let Some(cell) = self.get_cell(ri, ci) {
            if let Some(idx) = cell.style {
                if let Some(s) = self.styles.get(idx) {
                    return s.clone();
                }
            }
        }
        Style::default()
    }

    /// The merge range covering (ri, ci), if any.
    pub fn cell_merge(&self, ri: usize, ci: usize) -> Option<CellRange> {
        self.merges.get_first_includes(ri, ci).cloned()
    }

    /// Grow a selection rectangle so it fully contains every merge it touches
    /// (iterated to a fixed point, since growing can pull in further merges).
    pub fn expand_range_with_merges(
        &self,
        r0: usize,
        c0: usize,
        r1: usize,
        c1: usize,
    ) -> (usize, usize, usize, usize) {
        let (mut a, mut b, mut c, mut d) = (r0, c0, r1, c1);
        loop {
            let rect = CellRange::new(a, b, c, d);
            let mut changed = false;
            self.merges.for_each(|m| {
                if m.intersects(&rect) {
                    if m.sri < a {
                        a = m.sri;
                        changed = true;
                    }
                    if m.sci < b {
                        b = m.sci;
                        changed = true;
                    }
                    if m.eri > c {
                        c = m.eri;
                        changed = true;
                    }
                    if m.eci > d {
                        d = m.eci;
                        changed = true;
                    }
                }
            });
            if !changed {
                break;
            }
        }
        (a, b, c, d)
    }

    pub fn set_cell_style(&mut self, ri: usize, ci: usize, style_idx: usize) {
        let cell = self.get_cell_or_new(ri, ci);
        cell.set_style(style_idx);
    }

    pub fn set_cell(&mut self, ri: usize, ci: usize, cell: Cell) {
        self.rows.entry(ri).or_insert_with(Row::default).set_cell(ci, cell);
    }

    pub fn get_note(&self, ri: usize, ci: usize) -> Option<String> {
        self.get_cell(ri, ci).and_then(|c| c.note.clone())
    }

    pub fn set_note(&mut self, ri: usize, ci: usize, note: Option<String>) {
        self.get_cell_or_new(ri, ci).note = note;
    }

    pub fn delete_cell(&mut self, ri: usize, ci: usize) {
        if let Some(row) = self.rows.get_mut(&ri) {
            row.delete_cell(ci);
        }
    }

    /// Insert `n` blank rows at `at`, shifting existing rows down.
    pub fn insert_row(&mut self, at: usize, n: usize) {
        let mut new_rows = HashMap::new();
        for (ri, row) in self.rows.drain() {
            let nk = if ri >= at { ri + n } else { ri };
            new_rows.insert(nk, row);
        }
        self.rows = new_rows;
        self.row_count += n;
        self.merges.shift("row", at, n as isize, |_, _, _, _| {});
        self.adjust_all_formulas(true, at, n as isize, None);
    }

    /// Delete the row at `at`, shifting later rows up.
    pub fn delete_row(&mut self, at: usize) {
        let mut new_rows = HashMap::new();
        for (ri, row) in self.rows.drain() {
            if ri == at {
                continue;
            }
            let nk = if ri > at { ri - 1 } else { ri };
            new_rows.insert(nk, row);
        }
        self.rows = new_rows;
        self.row_count = self.row_count.saturating_sub(1);
        self.merges.shift("row", at, -1, |_, _, _, _| {});
        self.adjust_all_formulas(true, at + 1, -1, Some(at));
    }

    /// Insert `n` blank columns at `at`, shifting existing cells/cols right.
    pub fn insert_col(&mut self, at: usize, n: usize) {
        for row in self.rows.values_mut() {
            let mut nc = HashMap::new();
            for (ci, cell) in row.cells.drain() {
                let nk = if ci >= at { ci + n } else { ci };
                nc.insert(nk, cell);
            }
            row.cells = nc;
        }
        let mut new_cols = HashMap::new();
        for (ci, col) in self.cols.data.drain() {
            let nk = if ci >= at { ci + n } else { ci };
            new_cols.insert(nk, col);
        }
        self.cols.data = new_cols;
        self.cols.len += n;
        self.merges.shift("column", at, n as isize, |_, _, _, _| {});
        self.adjust_all_formulas(false, at, n as isize, None);
    }

    /// Delete the column at `at`, shifting later cells/cols left.
    pub fn delete_col(&mut self, at: usize) {
        for row in self.rows.values_mut() {
            let mut nc = HashMap::new();
            for (ci, cell) in row.cells.drain() {
                if ci == at {
                    continue;
                }
                let nk = if ci > at { ci - 1 } else { ci };
                nc.insert(nk, cell);
            }
            row.cells = nc;
        }
        let mut new_cols = HashMap::new();
        for (ci, col) in self.cols.data.drain() {
            if ci == at {
                continue;
            }
            let nk = if ci > at { ci - 1 } else { ci };
            new_cols.insert(nk, col);
        }
        self.cols.data = new_cols;
        self.cols.len = self.cols.len.saturating_sub(1);
        self.merges.shift("column", at, -1, |_, _, _, _| {});
        self.adjust_all_formulas(false, at + 1, -1, Some(at));
    }

    /// Rewrite cell references in every formula after a structural edit. Any
    /// reference whose row (or column, when `is_row` is false) index is
    /// `>= shift_from` is offset by `delta`.
    fn adjust_all_formulas(&mut self, is_row: bool, shift_from: usize, delta: isize, deleted: Option<usize>) {
        for row in self.rows.values_mut() {
            for cell in row.cells.values_mut() {
                if cell.text.starts_with('=') {
                    cell.text = adjust_formula_refs(&cell.text, is_row, shift_from, delta, deleted);
                }
            }
        }
    }

    pub fn get_row_height(&self, ri: usize) -> f64 {
        self.rows.get(&ri).map(|r| r.get_height()).unwrap_or(self.default_row_height)
    }

    pub fn set_row_height(&mut self, ri: usize, height: f64) {
        let row = self.rows.entry(ri).or_insert_with(Row::default);
        row.set_height(height);
    }

    pub fn set_col_width(&mut self, ci: usize, width: f64) {
        self.cols.set_width(ci, width);
    }

    pub fn get_col_width(&self, ci: usize) -> f64 {
        self.cols.get_width(ci)
    }

    pub fn copy(&mut self) {
        self.clipboard.copy(self.selector.range.clone());
    }

    pub fn cut(&mut self) {
        self.clipboard.cut(self.selector.range.clone());
    }

    pub fn clear_clipboard(&mut self) {
        self.clipboard.clear();
    }

    pub fn is_clipboard_clear(&self) -> bool {
        self.clipboard.is_clear()
    }

    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    pub fn set_selected_cell(&mut self, ri: usize, ci: usize) {
        self.selector.set_indexes(ri, ci);
        self.selector.range = CellRange::new(ri, ci, ri, ci);
    }

    pub fn set_selected_range(&mut self, range: CellRange) {
        self.selector.range = range;
    }

    pub fn get_selected_range(&self) -> &CellRange {
        &self.selector.range
    }

    pub fn is_single_selected(&self) -> bool {
        let (sri, sci, eri, eci) = (
            self.selector.range.sri,
            self.selector.range.sci,
            self.selector.range.eri,
            self.selector.range.eci,
        );
        if let Some(cell) = self.get_cell(sri, sci) {
            if let Some(merge) = &cell.merge {
                let (rn, cn) = *merge;
                return sri + rn == eri && sci + cn == eci;
            }
        }
        !self.selector.multiple()
    }

    pub fn freeze_is_active(&self) -> bool {
        self.freeze.0 > 0 || self.freeze.1 > 0
    }

    pub fn set_freeze(&mut self, ri: usize, ci: usize) {
        self.freeze = (ri, ci);
    }

    pub fn freeze_total_width(&self) -> f64 {
        self.cols.sum_width(0, self.freeze.1)
    }

    pub fn freeze_total_height(&self) -> f64 {
        let mut sum = 0.0;
        for i in 0..self.freeze.0 {
            sum += self.get_row_height(i);
        }
        sum
    }

    pub fn can_autofilter(&self) -> bool {
        !self.auto_filter.active()
    }

    pub fn autofilter(&mut self) {
        if self.auto_filter.active() {
            self.auto_filter.clear();
        } else {
            self.auto_filter.ref_ = Some(self.selector.range.to_string());
        }
    }

    pub fn add_style(&mut self, style: Style) -> usize {
        for (i, s) in self.styles.iter().enumerate() {
            if Self::styles_equal(s, &style) {
                return i;
            }
        }
        self.styles.push(style);
        self.styles.len() - 1
    }

    fn styles_equal(a: &Style, b: &Style) -> bool {
        a.bgcolor == b.bgcolor
            && a.color == b.color
            && a.align == b.align
            && a.valign == b.valign
            && a.text_wrap == b.text_wrap
            && a.underline == b.underline
            && a.strike == b.strike
            && a.bold == b.bold
            && a.italic == b.italic
            && a.font_size == b.font_size
            && a.font_family == b.font_family
            && a.format == b.format
    }

    pub fn merge(&mut self) {
        if self.is_single_selected() {
            return;
        }
        let (rn, cn) = self.selector.size();
        if rn > 1 || cn > 1 {
            let sri = self.selector.range.sri;
            let sci = self.selector.range.sci;
            let cell = self.get_cell_or_new(sri, sci);
            cell.merge = Some((rn - 1, cn - 1));
            self.merges.add(self.selector.range.clone());
            for ri in self.selector.range.sri..=self.selector.range.eri {
                for ci in self.selector.range.sci..=self.selector.range.eci {
                    if ri != sri || ci != sci {
                        if let Some(row) = self.rows.get_mut(&ri) {
                            row.delete_cell(ci);
                        }
                    }
                }
            }
        }
    }

    pub fn unmerge(&mut self) {
        if !self.is_single_selected() {
            return;
        }
        let sri = self.selector.range.sri;
        let sci = self.selector.range.sci;
        if let Some(row) = self.rows.get_mut(&sri) {
            row.delete_cell(sci);
        }
        self.merges.delete_within(&self.selector.range);
    }

    pub fn get_data(&self) -> serde_json::Value {
        serde_json::json!({
            "name": self.name,
            "freeze": crate::renderer::alphabets::xy2expr(self.freeze.1, self.freeze.0),
            "styles": self.styles,
            "merges": self.merges.get_data(),
            "rows": {
                "len": self.row_count,
                "_": serde_json::to_value(&self.rows).unwrap_or_default()
            },
            "cols": {
                "len": self.cols.len,
                "_": serde_json::to_value(&self.cols.data).unwrap_or_default()
            },
            "validations": self.validations.get_data(),
            "autofilter": self.auto_filter.get_data(),
        })
    }

    pub fn set_data(&mut self, data: serde_json::Value) {
        if let Some(name) = data.get("name").and_then(|v| v.as_str()) {
            self.name = name.to_string();
        }
        if let Some(freeze) = data.get("freeze").and_then(|v| v.as_str()) {
            if let ((x, y), _) = (crate::renderer::alphabets::exp2xy(freeze), true) {
                self.freeze = (y, x);
            }
        }
        if let Some(styles) = data.get("styles").and_then(|v| serde_json::from_value(v.clone()).ok()) {
            self.styles = styles;
        }
        if let Some(merges) = data.get("merges").and_then(|v| v.as_array()) {
            let merge_strings: Vec<String> = merges.iter().filter_map(|v| v.as_str().map(String::from)).collect();
            self.merges.set_data(merge_strings);
        }
        if let Some(rows_data) = data.get("rows").and_then(|v| v.as_object()) {
            if let Some(len) = rows_data.get("len").and_then(|v| v.as_u64()) {
                self.row_count = len as usize;
            }
            if let Some(rows_obj) = rows_data.get("_").and_then(|v| serde_json::from_value(v.clone()).ok()) {
                self.rows = rows_obj;
            }
        }
        if let Some(cols_data) = data.get("cols").and_then(|v| v.as_object()) {
            if let Some(len) = cols_data.get("len").and_then(|v| v.as_u64()) {
                self.cols.len = len as usize;
            }
            if let Some(cols_obj) = cols_data.get("_").and_then(|v| serde_json::from_value(v.clone()).ok()) {
                self.cols.data = cols_obj;
            }
        }
        if let Some(filter_data) = data.get("autofilter") {
            self.auto_filter.set_data(filter_data);
        }
    }

    pub fn get_data_json(&self) -> String {
        serde_json::to_string(&self.get_data()).unwrap_or_default()
    }

    pub fn set_data_json(&mut self, json: &str) {
        if let Ok(data) = serde_json::from_str(json) {
            self.set_data(data);
        }
    }
}

/// Rewrite the cell references inside a formula string after a row/column
/// insert or delete. A reference is `$?col$?row` (e.g. `A1`, `$A$1`, `$A1`,
/// `A$1`); `$` markers are preserved. The relative component shifts by `delta`
/// when its index is `>= shift_from`. On a delete, `deleted` carries the
/// removed index — references that point at it become `#REF!`.
fn adjust_formula_refs(
    text: &str,
    is_row: bool,
    shift_from: usize,
    delta: isize,
    deleted: Option<usize>,
) -> String {
    let re = Regex::new(r"(\$?)([A-Za-z]+)(\$?)([0-9]+)").unwrap();
    re.replace_all(text, |caps: &regex::Captures| {
        let col_lock = &caps[1];
        let row_lock = &caps[3];
        let col = index_at(&caps[2]);
        let row = caps[4].parse::<usize>().unwrap_or(1).saturating_sub(1);

        // A reference to the deleted row/column is invalidated.
        if let Some(d) = deleted {
            let idx = if is_row { row } else { col };
            if idx == d {
                return "#REF!".to_string();
            }
        }

        let mut new_col = col;
        let mut new_row = row;
        if is_row {
            if row_lock.is_empty() && row >= shift_from {
                new_row = (row as isize + delta).max(0) as usize;
            }
        } else if col_lock.is_empty() && col >= shift_from {
            new_col = (col as isize + delta).max(0) as usize;
        }

        format!("{}{}{}{}", col_lock, string_at(new_col), row_lock, new_row + 1)
    })
    .to_string()
}

/// Format a formula result for display: drop the fractional part for integers,
/// otherwise trim trailing zeros.
fn format_number(v: f64) -> String {
    if !v.is_finite() {
        return "#ERROR".to_string();
    }
    if (v - v.round()).abs() < 1e-9 {
        return format!("{}", v.round() as i64);
    }
    let s = format!("{:.6}", v);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    s.to_string()
}

/// A spreadsheet error value (`#DIV/0!`, etc.) produced during evaluation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EvalErr {
    Div0,
    Name,
    Value,
    Ref,
    Na,
    Num,
}

impl EvalErr {
    pub fn code(self) -> &'static str {
        match self {
            EvalErr::Div0 => "#DIV/0!",
            EvalErr::Name => "#NAME?",
            EvalErr::Value => "#VALUE!",
            EvalErr::Ref => "#REF!",
            EvalErr::Na => "#N/A",
            EvalErr::Num => "#NUM!",
        }
    }

    pub fn from_literal(s: &str) -> Option<EvalErr> {
        match s.trim() {
            "#DIV/0!" => Some(EvalErr::Div0),
            "#NAME?" => Some(EvalErr::Name),
            "#VALUE!" => Some(EvalErr::Value),
            "#REF!" => Some(EvalErr::Ref),
            "#N/A" => Some(EvalErr::Na),
            "#NUM!" => Some(EvalErr::Num),
            _ => None,
        }
    }
}

fn apply_function(name: &str, args: &[f64]) -> Result<f64, EvalErr> {
    let first = args.first().copied().unwrap_or(0.0);
    let second = args.get(1).copied().unwrap_or(0.0);
    let upper = name.to_uppercase();
    let v = match upper.as_str() {
        // Aggregation
        "SUM" => args.iter().sum(),
        "PRODUCT" => args.iter().product(),
        "AVERAGE" | "AVG" => {
            if args.is_empty() { 0.0 } else { args.iter().sum::<f64>() / args.len() as f64 }
        }
        "MAX" => finite_or(args.iter().cloned().fold(f64::NEG_INFINITY, f64::max)),
        "MIN" => finite_or(args.iter().cloned().fold(f64::INFINITY, f64::min)),
        "COUNT" => args.len() as f64,
        "SUMSQ" => args.iter().map(|v| v * v).sum(),
        "MEDIAN" => median(args),
        "VAR" => variance(args),
        "STDEV" => variance(args).sqrt(),

        // Logical (non-zero is truthy; returns 1.0/0.0)
        "AND" => bool_f64(!args.is_empty() && args.iter().all(|&v| v != 0.0)),
        "OR" => bool_f64(args.iter().any(|&v| v != 0.0)),
        "NOT" => bool_f64(first == 0.0),
        "TRUE" => 1.0,
        "FALSE" => 0.0,
        "IF" => {
            if args.len() >= 3 {
                if first != 0.0 { args[1] } else { args[2] }
            } else if args.len() == 2 {
                if first != 0.0 { args[1] } else { 0.0 }
            } else {
                0.0
            }
        }
        // IFS(cond1, val1, cond2, val2, …): first truthy condition's value.
        "IFS" => {
            let mut i = 0;
            let mut out = 0.0;
            while i + 1 < args.len() {
                if args[i] != 0.0 {
                    out = args[i + 1];
                    break;
                }
                i += 2;
            }
            out
        }

        // Math
        "ABS" => first.abs(),
        "SIGN" => {
            if first > 0.0 { 1.0 } else if first < 0.0 { -1.0 } else { 0.0 }
        }
        "MOD" => {
            if second == 0.0 { return Err(EvalErr::Div0); }
            first - second * (first / second).floor()
        }
        "POWER" => first.powf(second),
        "SQRT" => first.sqrt(),
        "EXP" => first.exp(),
        "LN" => first.ln(),
        "LOG10" => first.log10(),
        "LOG" => {
            if args.len() >= 2 { first.log(second) } else { first.log10() }
        }
        "INT" => first.floor(),
        "ROUND" => round_to(first, second),
        "ROUNDUP" => round_dir(first, second, true),
        "ROUNDDOWN" => round_dir(first, second, false),
        "CEILING" => {
            let sig = if args.len() >= 2 { second } else { 1.0 };
            if sig == 0.0 { 0.0 } else { (first / sig).ceil() * sig }
        }
        "FLOOR" => {
            let sig = if args.len() >= 2 { second } else { 1.0 };
            if sig == 0.0 { 0.0 } else { (first / sig).floor() * sig }
        }

        // Unknown function name.
        _ => return Err(EvalErr::Name),
    };

    // A finite-domain math function that produced NaN/∞ is a #NUM! error
    // (e.g. SQRT(-1), LN(0)).
    if matches!(upper.as_str(), "SQRT" | "LN" | "LOG" | "LOG10") && !v.is_finite() {
        return Err(EvalErr::Num);
    }
    Ok(v)
}

fn bool_f64(b: bool) -> f64 {
    if b { 1.0 } else { 0.0 }
}

fn round_to(v: f64, digits: f64) -> f64 {
    let factor = 10f64.powf(digits);
    (v * factor).round() / factor
}

/// ROUNDUP/ROUNDDOWN round away from / toward zero at `digits` places.
fn round_dir(v: f64, digits: f64, up: bool) -> f64 {
    let factor = 10f64.powf(digits);
    let scaled = v.abs() * factor;
    let r = if up { scaled.ceil() } else { scaled.floor() };
    (r / factor) * v.signum()
}

fn median(args: &[f64]) -> f64 {
    if args.is_empty() {
        return 0.0;
    }
    let mut v: Vec<f64> = args.to_vec();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    }
}

/// Sample variance (n-1 denominator), matching Excel's VAR.
fn variance(args: &[f64]) -> f64 {
    let n = args.len();
    if n < 2 {
        return 0.0;
    }
    let mean = args.iter().sum::<f64>() / n as f64;
    args.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (n - 1) as f64
}

/// Keep MAX/MIN sane on empty input (the fold seed is ±∞).
fn finite_or(v: f64) -> f64 {
    if v.is_finite() { v } else { 0.0 }
}
#[cfg(test)]
mod tests {
    use super::*;

    /// Build a sheet where each referenced cell holds the value `row + col`,
    /// mirroring the resolver `(x, y) => x + y` used in x-spreadsheet's
    /// cell_test.js, then evaluate a formula placed in a spare cell.
    fn eval(formula: &str, cells: &[(usize, usize)]) -> String {
        let mut d = DataProxy::new("t");
        for &(r, c) in cells {
            d.set_cell_text(r, c, &(r + c).to_string());
        }
        d.set_cell_text(50, 50, formula);
        d.cell_display_value(50, 50)
    }

    // Ported from x-spreadsheet test/core/cell_test.js (cell.render behavior).
    #[test]
    fn sum_plus_literals() {
        // =SUM(A1,B2,C1,C5)+50+B20 = 0+2+2+6+50+20
        let cells = [(0, 0), (1, 1), (0, 2), (4, 2), (19, 1)];
        assert_eq!(eval("=SUM(A1,B2, C1, C5) + 50 + B20", &cells), "80");
    }

    #[test]
    fn literal_plus_ref() {
        assert_eq!(eval("=50 + B20", &[(19, 1)]), "70");
    }

    #[test]
    fn if_with_comparison() {
        assert_eq!(eval("=IF(2>1, 2, 1)", &[]), "2");
        assert_eq!(eval("=IF(1>2, 2, 1)", &[]), "1");
        assert_eq!(eval("=IF(1=1, 7, 9)", &[]), "7");
    }

    #[test]
    fn average_range_with_arithmetic() {
        // =AVERAGE(A1:A3)+50*10-B20 = 1 + 500 - 20
        let cells = [(0, 0), (1, 0), (2, 0), (19, 1)];
        assert_eq!(eval("=AVERAGE(A1:A3) + 50 * 10 - B20", &cells), "481");
    }

    #[test]
    fn operator_precedence() {
        assert_eq!(eval("=1+2*3+(4*5+6)*7", &[]), "189");
        assert_eq!(eval("=10-5-20", &[]), "-15");
        assert_eq!(eval("=10-5*20", &[]), "-90");
    }

    // --- Absolute & mixed references (issue #3) ---

    #[test]
    fn absolute_refs_evaluate() {
        let mut d = DataProxy::new("t");
        d.set_cell_text(0, 0, "7"); // A1
        d.set_cell_text(2, 0, "=$A$1"); // A3
        d.set_cell_text(3, 0, "=A$1 + $A1"); // A4
        assert_eq!(d.cell_display_value(2, 0), "7");
        assert_eq!(d.cell_display_value(3, 0), "14");
    }

    #[test]
    fn insert_row_keeps_absolute_row() {
        let mut d = DataProxy::new("t");
        // =$A$1 + A2 : the absolute row stays, the relative one shifts down.
        d.set_cell_text(5, 0, "=$A$1 + A2");
        d.insert_row(0, 1);
        assert_eq!(d.get_cell_text(6, 0), "=$A$1 + A3");
    }

    #[test]
    fn insert_col_keeps_absolute_col() {
        let mut d = DataProxy::new("t");
        // =$A1 + B1 : the absolute column stays, the relative one shifts right.
        d.set_cell_text(0, 5, "=$A1 + B1");
        d.insert_col(0, 1);
        assert_eq!(d.get_cell_text(0, 6), "=$A1 + C1");
    }

    #[test]
    fn relative_refs_still_shift() {
        let mut d = DataProxy::new("t");
        d.set_cell_text(5, 0, "=SUM(B2:B3)");
        d.insert_row(0, 1);
        assert_eq!(d.get_cell_text(6, 0), "=SUM(B3:B4)");
    }

    // --- Expanded function library (issue #2) ---

    #[test]
    fn math_functions() {
        assert_eq!(eval("=POWER(2,10)", &[]), "1024");
        assert_eq!(eval("=SQRT(144)", &[]), "12");
        assert_eq!(eval("=MOD(10,3)", &[]), "1");
        assert_eq!(eval("=INT(7.9)", &[]), "7");
        assert_eq!(eval("=SIGN(-5)", &[]), "-1");
        assert_eq!(eval("=ROUNDUP(2.1,0)", &[]), "3");
        assert_eq!(eval("=ROUNDDOWN(2.9,0)", &[]), "2");
        assert_eq!(eval("=CEILING(12,5)", &[]), "15");
        assert_eq!(eval("=FLOOR(12,5)", &[]), "10");
        assert_eq!(eval("=SUMSQ(3,4)", &[]), "25");
    }

    #[test]
    fn logical_functions() {
        assert_eq!(eval("=AND(1,1,1)", &[]), "1");
        assert_eq!(eval("=AND(1,0)", &[]), "0");
        assert_eq!(eval("=OR(0,0,1)", &[]), "1");
        assert_eq!(eval("=NOT(0)", &[]), "1");
        assert_eq!(eval("=IF(AND(2>1, 3>2), 10, 20)", &[]), "10");
        assert_eq!(eval("=IFS(0, 1, 1, 42, 1, 99)", &[]), "42");
    }

    #[test]
    fn stats_functions() {
        assert_eq!(eval("=MEDIAN(1,2,3,4)", &[]), "2.5");
        assert_eq!(eval("=MEDIAN(5,1,3)", &[]), "3");
        assert_eq!(eval("=MAX()", &[]), "0"); // empty is safe
        assert_eq!(eval("=VAR(2,4,6)", &[]), "4"); // sample variance
    }

    // --- Error values & propagation (issue #5) ---

    #[test]
    fn error_values() {
        assert_eq!(eval("=1/0", &[]), "#DIV/0!");
        assert_eq!(eval("=5/(2-2)", &[]), "#DIV/0!");
        assert_eq!(eval("=NOPE(1,2)", &[]), "#NAME?");
        assert_eq!(eval("=SQRT(-1)", &[]), "#NUM!");
        assert_eq!(eval("=MOD(5,0)", &[]), "#DIV/0!");
    }

    #[test]
    fn errors_propagate_through_refs() {
        let mut d = DataProxy::new("t");
        d.set_cell_text(0, 0, "=1/0"); // A1 -> #DIV/0!
        d.set_cell_text(1, 0, "=A1+1"); // A2 references the error
        assert_eq!(d.cell_display_value(0, 0), "#DIV/0!");
        assert_eq!(d.cell_display_value(1, 0), "#DIV/0!");
    }

    #[test]
    fn error_literal_displayed_and_propagated() {
        let mut d = DataProxy::new("t");
        d.set_cell_text(0, 0, "#N/A"); // literal error value
        d.set_cell_text(1, 0, "=A1*2");
        assert_eq!(d.cell_display_value(0, 0), "#N/A");
        assert_eq!(d.cell_display_value(1, 0), "#N/A");
    }

    #[test]
    fn delete_row_makes_ref_error() {
        let mut d = DataProxy::new("t");
        d.set_cell_text(0, 0, "=A3+B1"); // A1 references A3 (row index 2)
        d.delete_row(2); // remove the row A3 lives in
        assert_eq!(d.get_cell_text(0, 0), "=#REF!+B1");
        assert_eq!(d.cell_display_value(0, 0), "#REF!");
    }

    #[test]
    fn delete_col_makes_ref_error() {
        let mut d = DataProxy::new("t");
        d.set_cell_text(0, 0, "=C1+1"); // references column C (index 2)
        d.delete_col(2);
        assert_eq!(d.get_cell_text(0, 0), "=#REF!+1");
        assert_eq!(d.cell_display_value(0, 0), "#REF!");
    }
}
