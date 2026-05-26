use serde::{Serialize, Deserialize};
use std::collections::{HashMap, HashSet};
use crate::core::cell_range::CellRange;
use crate::formula::parser::{tokenize, Token};
use crate::renderer::alphabets::{exp2xy, xy2expr};
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
        let raw = if let Some(expr) = text.strip_prefix('=') {
            let mut visited = HashSet::new();
            visited.insert((ri, ci));
            match self.eval_expr(expr, &mut visited) {
                Ok(v) => format_number(v),
                Err(_) => return "#ERROR".to_string(),
            }
        } else {
            text
        };
        // Apply the cell's display format (number/currency/percent/…).
        let fmt = self.get_cell_style(ri, ci).format;
        crate::core::format::format_value(&raw, &fmt)
    }

    /// Resolve a cell to a numeric value for use inside a formula. Non-numeric
    /// text resolves to 0; nested formulas recurse with a circular-ref guard.
    fn resolve_numeric(&self, ri: usize, ci: usize, visited: &mut HashSet<(usize, usize)>) -> f64 {
        if visited.contains(&(ri, ci)) {
            return 0.0;
        }
        let text = self.get_cell_text(ri, ci);
        if let Some(expr) = text.strip_prefix('=') {
            visited.insert((ri, ci));
            let v = self.eval_expr(expr, visited).unwrap_or(0.0);
            visited.remove(&(ri, ci));
            v
        } else {
            text.trim().parse::<f64>().unwrap_or(0.0)
        }
    }

    fn eval_expr(&self, expr: &str, visited: &mut HashSet<(usize, usize)>) -> Result<f64, String> {
        let tokens = tokenize(expr);
        let mut pos = 0usize;
        let v = self.parse_add(&tokens, &mut pos, visited)?;
        Ok(v)
    }

    // expr := term (('+' | '-') term)*
    fn parse_add(&self, t: &[Token], pos: &mut usize, vis: &mut HashSet<(usize, usize)>) -> Result<f64, String> {
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
    fn parse_mul(&self, t: &[Token], pos: &mut usize, vis: &mut HashSet<(usize, usize)>) -> Result<f64, String> {
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
                            return Err("div by zero".to_string());
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
    fn parse_factor(&self, t: &[Token], pos: &mut usize, vis: &mut HashSet<(usize, usize)>) -> Result<f64, String> {
        let tok = t.get(*pos).ok_or("unexpected end")?.clone();
        match tok {
            Token::Number(n) => {
                *pos += 1;
                Ok(n)
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
                let v = self.parse_add(t, pos, vis)?;
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
                Ok(apply_function(&name, &args))
            }
            Token::CellRef(r) => {
                *pos += 1;
                let (c, row) = exp2xy(&r);
                Ok(self.resolve_numeric(row, c, vis))
            }
            Token::String(s) => {
                *pos += 1;
                Ok(s.trim().parse::<f64>().unwrap_or(0.0))
            }
            other => Err(format!("unexpected token {:?}", other)),
        }
    }

    // Parse a comma-separated argument list (until RightParen), flattening
    // any `A1:B3` ranges into the individual cell values they cover.
    fn parse_args(&self, t: &[Token], pos: &mut usize, vis: &mut HashSet<(usize, usize)>) -> Result<Vec<f64>, String> {
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
                        args.push(self.resolve_numeric(r, c, vis));
                    }
                }
                *pos += 3;
            } else {
                args.push(self.parse_add(t, pos, vis)?);
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
        self.adjust_all_formulas(true, at, n as isize);
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
        self.adjust_all_formulas(true, at + 1, -1);
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
        self.adjust_all_formulas(false, at, n as isize);
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
        self.adjust_all_formulas(false, at + 1, -1);
    }

    /// Rewrite cell references in every formula after a structural edit. Any
    /// reference whose row (or column, when `is_row` is false) index is
    /// `>= shift_from` is offset by `delta`.
    fn adjust_all_formulas(&mut self, is_row: bool, shift_from: usize, delta: isize) {
        for row in self.rows.values_mut() {
            for cell in row.cells.values_mut() {
                if cell.text.starts_with('=') {
                    cell.text = adjust_formula_refs(&cell.text, is_row, shift_from, delta);
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
/// insert or delete. A reference is `[A-Za-z]+[0-9]+`; we shift its row (or
/// column when `is_row` is false) by `delta` when the index is `>= shift_from`.
fn adjust_formula_refs(text: &str, is_row: bool, shift_from: usize, delta: isize) -> String {
    let re = Regex::new(r"[A-Za-z]+[0-9]+").unwrap();
    re.replace_all(text, |caps: &regex::Captures| {
        let r = &caps[0];
        let (col, row) = exp2xy(r);
        if is_row {
            if row >= shift_from {
                let nr = (row as isize + delta).max(0) as usize;
                return xy2expr(col, nr);
            }
        } else if col >= shift_from {
            let nc = (col as isize + delta).max(0) as usize;
            return xy2expr(nc, row);
        }
        r.to_string()
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

fn apply_function(name: &str, args: &[f64]) -> f64 {
    match name.to_uppercase().as_str() {
        "SUM" => args.iter().sum(),
        "PRODUCT" => args.iter().product(),
        "AVERAGE" | "AVG" => {
            if args.is_empty() { 0.0 } else { args.iter().sum::<f64>() / args.len() as f64 }
        }
        "MAX" => args.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
        "MIN" => args.iter().cloned().fold(f64::INFINITY, f64::min),
        "COUNT" => args.len() as f64,
        "ABS" => args.first().map(|v| v.abs()).unwrap_or(0.0),
        "ROUND" => {
            let v = args.first().copied().unwrap_or(0.0);
            let digits = args.get(1).copied().unwrap_or(0.0);
            let factor = 10f64.powf(digits);
            (v * factor).round() / factor
        }
        "IF" => {
            if args.len() >= 3 {
                if args[0] != 0.0 { args[1] } else { args[2] }
            } else {
                0.0
            }
        }
        _ => 0.0,
    }
}