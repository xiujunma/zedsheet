use serde::{Serialize, Deserialize};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::{Rc, Weak};
use crate::core::cell_range::CellRange;
use crate::formula::parser::{tokenize, Token};
use crate::renderer::alphabets::{exp2xy, index_at, string_at};
use regex::Regex;
use crate::core::cell::Cell;
use crate::core::row::Row;
use crate::core::col::{Col, Cols};
use crate::core::merges::Merges;
use crate::core::state::{Selector, Scroll, Clipboard, History};
use crate::core::validation::{Validation, Validations};
use crate::core::auto_filter::AutoFilter;

/// Shared registry of every sheet's `DataProxy`. Held by `ZedSheet` and
/// referenced by every `DataProxy` so cross-sheet formulas can resolve
/// `Sheet2!A1` against the right sheet (issue #4).
pub type SheetsRegistry = Rc<RefCell<Vec<DataProxy>>>;

/// The circular-reference guard for formula evaluation. Keyed by
/// `(sheet_name, row, col)` — the sheet component is essential so a cross-sheet
/// cycle (`Sheet1!A1 = Sheet2!A1`, `Sheet2!A1 = Sheet1!A1`) is detected instead
/// of recursing forever (issue #4). The set is threaded *through* cross-sheet
/// hops rather than reset, so a reference back to an in-progress cell is caught.
type Visited = HashSet<(String, usize, usize)>;

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
    /// Text rotation in degrees, `Some(angle)`. `None` (or `Some(0)`) is the
    /// default unrotated layout. Positive values rotate clockwise. Excel
    /// conventionally uses -90 to 90; the renderer happily draws any angle
    /// (issue #25).
    #[serde(default)]
    pub rotation: Option<f64>,
    /// When `true`, the renderer shrinks the font size until the text fits
    /// inside the cell without wrapping. No-op for empty cells or cells with
    /// `text_wrap` enabled (issue #25).
    #[serde(default)]
    pub shrink_to_fit: bool,
    /// Left indent in CSS pixels, added on top of the cell's standard
    /// padding. Excel uses a small fixed step (1 unit ≈ one character width);
    /// we use raw pixels so callers can step freely (issue #25).
    #[serde(default)]
    pub indent: usize,
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
            rotation: None,
            shrink_to_fit: false,
            indent: 0,
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
    /// Named ranges (sheet-scoped): UPPERCASE name → range expression like
    /// `"B2:B3"` or `"B2"`. Resolved by the evaluator and the name box.
    pub named_ranges: HashMap<String, String>,
    /// All sheets in the workbook, used to resolve `Sheet2!A1` references
    /// (issue #4). `None` for tests / standalone use that don't need
    /// cross-sheet refs; `ZedSheet` wires this up at construction time.
    ///
    /// Stored as a `Weak` on purpose: each sheet lives *inside* this same
    /// `Vec`, so a strong `Rc` here would form a reference cycle and leak the
    /// whole workbook. `ZedSheet` (and the undo stacks) hold the strong `Rc`.
    pub sheets: Option<Weak<RefCell<Vec<DataProxy>>>>,
    /// When `true`, every cell in this sheet is read-only — editor, paste,
    /// and clear are all blocked. Toggleable at runtime via
    /// `set_read_only` (issue #24). Wrapped in `Rc<RefCell>` so every
    /// `DataProxy` clone (the registry entry *and* the renderer's active
    /// copy) shares a single source of truth — toggling on either side
    /// is immediately visible to the other.
    pub read_only: Rc<RefCell<bool>>,
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
            named_ranges: HashMap::new(),
            sheets: None,
            read_only: Rc::new(RefCell::new(false)),
        }
    }
}

impl DataProxy {
    pub fn new(name: &str) -> Self {
        let mut dp = DataProxy::default();
        dp.name = name.to_string();
        dp
    }

    /// Wire the workbook-wide sheets registry used to resolve `Sheet2!A1`
    /// cross-sheet references (issue #4). Call once per `DataProxy` after the
    /// workbook's `Vec<DataProxy>` is built; the registry is shared via `Rc`
    /// so a single `set_sheets` on one `DataProxy` doesn't propagate to
    /// siblings — wire them all explicitly.
    pub fn set_sheets(&mut self, sheets: &SheetsRegistry) {
        // Downgrade to a Weak so this back-reference doesn't keep the workbook
        // (which contains this very DataProxy) alive — see the field docs.
        self.sheets = Some(Rc::downgrade(sheets));
    }

    /// Put the sheet in read-only mode. While `true`, the editor refuses to
    /// open, paste/clear are blocked, and `is_cell_editable` returns false
    /// for every cell (issue #24). Read the current value with `is_read_only`.
    pub fn set_read_only(&mut self, read_only: bool) {
        *self.read_only.borrow_mut() = read_only;
    }

    /// `true` when the sheet is in read-only mode. Per-cell locking is
    /// separate — see `is_cell_editable` for the combined check.
    pub fn is_read_only(&self) -> bool {
        *self.read_only.borrow()
    }

    /// `true` when a write to `(ri, ci)` is allowed. Combines the sheet-wide
    /// read-only flag with the cell's own `editable` flag (issue #24): a
    /// cell with no explicit value defaults to editable, matching `Cell`'s
    /// `Default` impl.
    pub fn is_cell_editable(&self, ri: usize, ci: usize) -> bool {
        if *self.read_only.borrow() {
            return false;
        }
        self.get_cell(ri, ci).map(|c| c.editable).unwrap_or(true)
    }

    /// Toggle the per-cell `editable` flag (issue #24). Setting `false`
    /// locks the cell even when the sheet is editable; setting `true`
    /// re-allows edits to it.
    pub fn set_cell_editable(&mut self, ri: usize, ci: usize, editable: bool) {
        let cell = self.get_cell_or_new(ri, ci);
        cell.editable = editable;
    }

    /// Look up a sheet by name (case-insensitive) in the workbook registry.
    /// Returns `None` if no registry is wired or no sheet with that name
    /// exists, which the evaluator surfaces as `#REF!`.
    fn find_sheet(&self, name: &str) -> Option<DataProxy> {
        let reg = self.sheets.as_ref()?.upgrade()?;
        let upper = name.to_uppercase();
        let found = reg
            .borrow()
            .iter()
            .find(|d| d.name.to_uppercase() == upper)
            .cloned();
        found
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
        // Defense-in-depth: refuse writes to a locked cell or a read-only
        // sheet (issue #24). Callers that have already checked may still
        // pass through; this is a safety net.
        if !self.is_cell_editable(ri, ci) {
            return;
        }
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
            let mut visited: Visited = HashSet::new();
            visited.insert((self.name.clone(), ri, ci));
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
    fn resolve_numeric(&self, ri: usize, ci: usize, visited: &mut Visited) -> Result<f64, EvalErr> {
        // Key by sheet so the same (row, col) on different sheets don't collide
        // and a cross-sheet cycle is detected (issue #4).
        let key = (self.name.clone(), ri, ci);
        if visited.contains(&key) {
            return Ok(0.0);
        }
        let text = self.get_cell_text(ri, ci);
        if let Some(e) = EvalErr::from_literal(&text) {
            return Err(e);
        }
        if let Some(expr) = text.strip_prefix('=') {
            visited.insert(key.clone());
            let v = self.eval_expr(expr, visited);
            visited.remove(&key);
            v
        } else {
            // Plain numbers first; otherwise a date string resolves to its serial
            // so date arithmetic (e.g. `=A1+1`) and date functions work.
            let t = text.trim();
            Ok(t.parse::<f64>()
                .unwrap_or_else(|_| crate::core::date::parse_date(t).unwrap_or(0.0)))
        }
    }

    fn eval_expr(&self, expr: &str, visited: &mut Visited) -> Result<f64, EvalErr> {
        let tokens = tokenize(expr);
        let mut pos = 0usize;
        let v = self.parse_cmp(&tokens, &mut pos, visited)?;
        Ok(v)
    }

    // expr := add ((= | == | > | < | >= | <=) add)*  — comparisons yield 1.0/0.0
    fn parse_cmp(&self, t: &[Token], pos: &mut usize, vis: &mut Visited) -> Result<f64, EvalErr> {
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
    fn parse_add(&self, t: &[Token], pos: &mut usize, vis: &mut Visited) -> Result<f64, EvalErr> {
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
    fn parse_mul(&self, t: &[Token], pos: &mut usize, vis: &mut Visited) -> Result<f64, EvalErr> {
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
    fn parse_factor(&self, t: &[Token], pos: &mut usize, vis: &mut Visited) -> Result<f64, EvalErr> {
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
            Token::SheetCellRef { sheet, ref_ } => {
                *pos += 1;
                let (c, row) = exp2xy(&ref_);
                // Cross-sheet ref: resolve on the named sheet. An unknown
                // sheet is a #REF! error (issue #4).
                if let Some(target) = self.find_sheet(&sheet) {
                    // Thread the SAME visited set through (keyed by sheet name),
                    // so a cross-sheet cycle is caught instead of recursing
                    // forever (issue #4).
                    target.resolve_numeric(row, c, vis)
                } else {
                    Err(EvalErr::Ref)
                }
            }
            Token::Name(n) => {
                *pos += 1;
                // Scalar context: a named range resolves to its top-left cell;
                // an undefined name is a #NAME? error.
                match self.resolve_name(&n) {
                    Some((r0, c0, _, _)) => self.resolve_numeric(r0, c0, vis),
                    None => Err(EvalErr::Name),
                }
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
    fn parse_args(&self, t: &[Token], pos: &mut usize, vis: &mut Visited) -> Result<Vec<f64>, EvalErr> {
        let mut args = Vec::new();
        if matches!(t.get(*pos), Some(Token::RightParen)) {
            *pos += 1;
            return Ok(args);
        }
        loop {
            // Sheet-qualified range: `Sheet2!A1:B3` (issue #4).
            if let Some(Token::SheetRange { sheet, from, to }) = t.get(*pos).cloned() {
                let target = self.find_sheet(&sheet).ok_or(EvalErr::Ref)?;
                let (c0, r0) = exp2xy(&from);
                let (c1, r1) = exp2xy(&to);
                let (r0, r1) = (r0.min(r1), r0.max(r1));
                let (c0, c1) = (c0.min(c1), c0.max(c1));
                // Thread the same sheet-keyed visited set through (issue #4).
                for r in r0..=r1 {
                    for c in c0..=c1 {
                        args.push(target.resolve_numeric(r, c, vis)?);
                    }
                }
                *pos += 1;
            // Range: CellRef ':' CellRef
            } else if let (Some(Token::CellRef(a)), Some(Token::Colon), Some(Token::CellRef(b))) =
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
            } else if let Some(Token::Name(n)) = t.get(*pos).cloned() {
                // A bare named range as an argument expands to its cells; a name
                // inside a larger expression (e.g. `Rev*2`) falls through to the
                // scalar handling in parse_cmp/parse_factor.
                let bare = matches!(
                    t.get(*pos + 1),
                    Some(Token::Comma) | Some(Token::RightParen) | None
                );
                if bare {
                    match self.resolve_name(&n) {
                        Some((r0, c0, r1, c1)) => {
                            for r in r0..=r1 {
                                for c in c0..=c1 {
                                    args.push(self.resolve_numeric(r, c, vis)?);
                                }
                            }
                            *pos += 1;
                        }
                        None => return Err(EvalErr::Name),
                    }
                } else {
                    args.push(self.parse_cmp(t, pos, vis)?);
                }
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

    pub fn get_link(&self, ri: usize, ci: usize) -> Option<String> {
        self.get_cell(ri, ci).and_then(|c| c.link.clone())
    }

    pub fn set_link(&mut self, ri: usize, ci: usize, link: Option<String>) {
        self.get_cell_or_new(ri, ci).link = link;
    }

    // --- Named ranges (issue #21) ---

    /// Define (or replace) a sheet-scoped named range. Names are case-insensitive.
    pub fn set_named_range(&mut self, name: &str, range_expr: &str) {
        self.named_ranges.insert(name.to_uppercase(), range_expr.to_string());
    }

    /// The range expression for a name (e.g. `"B2:B3"`), if defined.
    pub fn get_named_range(&self, name: &str) -> Option<String> {
        self.named_ranges.get(&name.to_uppercase()).cloned()
    }

    /// Remove a named range; returns whether it existed.
    pub fn remove_named_range(&mut self, name: &str) -> bool {
        self.named_ranges.remove(&name.to_uppercase()).is_some()
    }

    /// Inclusive cell bounds `(r0, c0, r1, c1)` for a defined name (for the name
    /// box to select it), or `None` if undefined.
    pub fn named_range_bounds(&self, name: &str) -> Option<(usize, usize, usize, usize)> {
        self.resolve_name(name)
    }

    /// Resolve a name to inclusive cell bounds `(r0, c0, r1, c1)`, or `None` if
    /// the name is undefined.
    fn resolve_name(&self, name: &str) -> Option<(usize, usize, usize, usize)> {
        let expr = self.named_ranges.get(&name.to_uppercase())?;
        Some(parse_range_expr(expr))
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
            && a.rotation == b.rotation
            && a.shrink_to_fit == b.shrink_to_fit
            && a.indent == b.indent
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
            "namedRanges": serde_json::to_value(&self.named_ranges).unwrap_or_default(),
        })
    }

    pub fn set_data(&mut self, data: serde_json::Value) {
        if let Some(name) = data.get("name").and_then(|v| v.as_str()) {
            self.name = name.to_string();
        }
        if let Some(freeze) = data.get("freeze").and_then(|v| v.as_str()) {
            let (x, y) = crate::renderer::alphabets::exp2xy(freeze);
            self.freeze = (y, x);
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
        if let Some(v) = data.get("validations").cloned() {
            if let Ok(list) = serde_json::from_value::<Vec<Validation>>(v) {
                self.validations.set_data(list);
            }
        }
        if let Some(nr) = data
            .get("namedRanges")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
        {
            self.named_ranges = nr;
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
///
/// Cross-sheet prefixes (`Sheet2!A1`, issue #4) are masked out before the
/// cell-ref substitution so the regex doesn't mistake the sheet name for a
/// cell ref, and restored afterwards.
fn adjust_formula_refs(
    text: &str,
    is_row: bool,
    shift_from: usize,
    delta: isize,
    deleted: Option<usize>,
) -> String {
    let (masked, placeholders) = mask_sheet_prefixes(text);
    let re = Regex::new(r"(\$?)([A-Za-z]+)(\$?)([0-9]+)").unwrap();
    let shifted = re
        .replace_all(&masked, |caps: &regex::Captures| {
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
        .to_string();
    restore_placeholders(&shifted, &placeholders)
}

/// Shift every *relative* component of a formula's cell references by
/// (`drow`, `dcol`) — the copy/fill transform. `$`-anchored components stay
/// put. Sheet prefixes are masked out so the regex only touches refs
/// (issue #4).
fn shift_formula_refs(text: &str, drow: isize, dcol: isize) -> String {
    let (masked, placeholders) = mask_sheet_prefixes(text);
    let re = Regex::new(r"(\$?)([A-Za-z]+)(\$?)([0-9]+)").unwrap();
    let shifted = re
        .replace_all(&masked, |caps: &regex::Captures| {
            let col_lock = &caps[1];
            let row_lock = &caps[3];
            let col = index_at(&caps[2]);
            let row = caps[4].parse::<usize>().unwrap_or(1).saturating_sub(1);
            let new_col = if col_lock.is_empty() {
                (col as isize + dcol).max(0) as usize
            } else {
                col
            };
            let new_row = if row_lock.is_empty() {
                (row as isize + drow).max(0) as usize
            } else {
                row
            };
            format!("{}{}{}{}", col_lock, string_at(new_col), row_lock, new_row + 1)
        })
        .to_string();
    restore_placeholders(&shifted, &placeholders)
}

/// Replace every `SheetName!` prefix in `text` with a unique ASCII-private-use
/// placeholder, returning the masked text plus the (placeholder → original)
/// table used by `restore_placeholders`. The placeholder is shaped so the
/// cell-ref regex (`\$?[A-Za-z]+\$?[0-9]+`) cannot match any of its
/// characters: it's bracketed by SOH control bytes and `#`s, with a pure-digit
/// index between them — no `[A-Za-z]+[0-9]+` substring ever appears.
fn mask_sheet_prefixes(text: &str) -> (String, Vec<(String, String)>) {
    let re = Regex::new(r"[A-Za-z_][A-Za-z0-9_]*!").unwrap();
    let mut placeholders: Vec<(String, String)> = Vec::new();
    let masked = re
        .replace_all(text, |caps: &regex::Captures| {
            // \x01#<idx>#\x01 — neither the control bytes nor `#` are
            // `[A-Za-z]` or `[0-9]`, so the cell-ref regex's character
            // classes reject every char of the placeholder.
            let key = format!("\u{0001}#{}#\u{0001}", placeholders.len());
            placeholders.push((caps[0].to_string(), key.clone()));
            key
        })
        .to_string();
    (masked, placeholders)
}

/// Inverse of `mask_sheet_prefixes`: substitute each placeholder back with
/// the original sheet prefix. Iterating in reverse keeps earlier
/// placeholders from being clobbered if a later one happened to embed an
/// earlier one's text (they don't, but reverse is the safe order).
fn restore_placeholders(text: &str, placeholders: &[(String, String)]) -> String {
    let mut out = text.to_string();
    for (orig, key) in placeholders.iter().rev() {
        out = out.replace(key, orig);
    }
    out
}

/// Compute the filled text for one line of a fill-handle drag. `source` is the
/// source cells' text in fill order; `n` target cells follow them.
///
/// - All-numeric source of length ≥2 → continue the arithmetic series.
/// - Otherwise tile the source cyclically; formula cells have their relative
///   references shifted along the fill axis (`axis_is_row` ⇒ shift rows).
pub fn fill_line(source: &[String], n: usize, axis_is_row: bool) -> Vec<String> {
    let len = source.len();
    if len == 0 {
        return Vec::new();
    }
    // Numeric series when every source cell is a plain number and there are ≥2.
    if len >= 2 {
        let nums: Option<Vec<f64>> =
            source.iter().map(|s| s.trim().parse::<f64>().ok()).collect();
        if let Some(nums) = nums {
            let step = (nums[len - 1] - nums[0]) / (len as f64 - 1.0);
            let last = nums[len - 1];
            return (0..n)
                .map(|i| format_number(last + step * (i as f64 + 1.0)))
                .collect();
        }
    }
    // Tile/copy, shifting formula references for each step past the source.
    (0..n)
        .map(|i| {
            let src = &source[i % len];
            if src.starts_with('=') {
                let shift = (len * (i / len + 1)) as isize;
                if axis_is_row {
                    shift_formula_refs(src, shift, 0)
                } else {
                    shift_formula_refs(src, 0, shift)
                }
            } else {
                src.clone()
            }
        })
        .collect()
}

/// Format a formula result for display: drop the fractional part for integers,
/// otherwise trim trailing zeros.
/// Parse a range expression like `"B2:B3"` or `"B2"` into inclusive cell bounds
/// `(r0, c0, r1, c1)`.
fn parse_range_expr(expr: &str) -> (usize, usize, usize, usize) {
    let expr = expr.trim();
    if let Some((a, b)) = expr.split_once(':') {
        let (c0, r0) = exp2xy(a.trim());
        let (c1, r1) = exp2xy(b.trim());
        (r0.min(r1), c0.min(c1), r0.max(r1), c0.max(c1))
    } else {
        let (c, r) = exp2xy(expr);
        (r, c, r, c)
    }
}

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

        // Date & time (serial numbers; see core::date)
        "DATE" => crate::core::date::to_serial(first as i64, second as i64, args.get(2).copied().unwrap_or(1.0) as i64),
        "YEAR" => crate::core::date::from_serial(first).0 as f64,
        "MONTH" => crate::core::date::from_serial(first).1 as f64,
        "DAY" => crate::core::date::from_serial(first).2 as f64,
        "HOUR" => crate::core::date::time_parts(first).0 as f64,
        "MINUTE" => crate::core::date::time_parts(first).1 as f64,
        "SECOND" => crate::core::date::time_parts(first).2 as f64,
        // Excel default WEEKDAY: 1 = Sunday … 7 = Saturday.
        "WEEKDAY" => ((first.floor() as i64 - 1).rem_euclid(7) + 1) as f64,
        "TODAY" => today_serial(),
        "NOW" => now_serial(),

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

/// Today's date as a serial number (local time). `TODAY()` in Excel.
#[cfg(target_arch = "wasm32")]
fn today_serial() -> f64 {
    let d = js_sys::Date::new_0();
    crate::core::date::to_serial(
        d.get_full_year() as i64,
        d.get_month() as i64 + 1, // JS months are 0-based
        d.get_date() as i64,
    )
}

/// The current date and time as a serial number (local time). `NOW()` in Excel.
#[cfg(target_arch = "wasm32")]
fn now_serial() -> f64 {
    let d = js_sys::Date::new_0();
    let day = crate::core::date::to_serial(
        d.get_full_year() as i64,
        d.get_month() as i64 + 1,
        d.get_date() as i64,
    );
    let secs = d.get_hours() as f64 * 3600.0 + d.get_minutes() as f64 * 60.0 + d.get_seconds() as f64;
    day + secs / 86_400.0
}

// Native (test) builds have no JS clock; these are never exercised by tests.
#[cfg(not(target_arch = "wasm32"))]
fn today_serial() -> f64 {
    0.0
}
#[cfg(not(target_arch = "wasm32"))]
fn now_serial() -> f64 {
    0.0
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

    // --- Date & time values (issue #6) ---

    #[test]
    fn date_functions() {
        assert_eq!(eval("=DATE(2024,1,15)", &[]), "45306"); // serial number
        assert_eq!(eval("=YEAR(DATE(2024,1,15))", &[]), "2024");
        assert_eq!(eval("=MONTH(DATE(2024,3,1))", &[]), "3");
        assert_eq!(eval("=DAY(DATE(2024,3,15))", &[]), "15");
        // Out-of-range month rolls into the next year, like Excel's DATE().
        assert_eq!(eval("=YEAR(DATE(2024,13,1))", &[]), "2025");
    }

    #[test]
    fn date_time_component_functions() {
        assert_eq!(eval("=HOUR(0.5)", &[]), "12"); // noon
        assert_eq!(eval("=MINUTE(0.25)", &[]), "0"); // 06:00:00
        assert_eq!(eval("=SECOND(0.5)", &[]), "0");
        assert_eq!(eval("=WEEKDAY(DATE(2024,1,15))", &[]), "2"); // a Monday -> 2
    }

    #[test]
    fn date_arithmetic_through_refs() {
        let mut d = DataProxy::new("t");
        d.set_cell_text(0, 0, "2024-01-15"); // A1 holds a date string
        d.set_cell_text(1, 0, "=DAY(A1+1)"); // the day-of-month of the next day
        assert_eq!(d.cell_display_value(1, 0), "16");
        d.set_cell_text(2, 0, "=YEAR(A1)");
        assert_eq!(d.cell_display_value(2, 0), "2024");
    }

    // --- Hyperlinks (issue #23) ---

    #[test]
    fn link_get_set_round_trip() {
        let mut d = DataProxy::new("t");
        assert_eq!(d.get_link(0, 0), None);
        d.set_cell_text(0, 0, "Docs");
        d.set_link(0, 0, Some("https://example.com".to_string()));
        assert_eq!(d.get_link(0, 0), Some("https://example.com".to_string()));
        // Editing the cell text preserves the link.
        d.set_cell_text(0, 0, "Documentation");
        assert_eq!(d.get_cell_text(0, 0), "Documentation");
        assert_eq!(d.get_link(0, 0), Some("https://example.com".to_string()));
        // Clearing.
        d.set_link(0, 0, None);
        assert_eq!(d.get_link(0, 0), None);
    }

    // --- Named ranges (issue #21) ---

    #[test]
    fn named_range_in_function() {
        let mut d = DataProxy::new("t");
        d.set_cell_text(1, 1, "10"); // B2
        d.set_cell_text(2, 1, "20"); // B3
        d.set_named_range("Revenue", "B2:B3");
        d.set_cell_text(0, 0, "=SUM(Revenue)");
        assert_eq!(d.cell_display_value(0, 0), "30");
    }

    #[test]
    fn named_single_cell_as_value() {
        let mut d = DataProxy::new("t");
        d.set_cell_text(1, 1, "10"); // B2
        d.set_named_range("Price", "B2");
        d.set_cell_text(0, 0, "=Price*2");
        assert_eq!(d.cell_display_value(0, 0), "20");
    }

    #[test]
    fn named_range_mixed_with_literal_arg() {
        let mut d = DataProxy::new("t");
        d.set_cell_text(1, 1, "10"); // B2
        d.set_cell_text(2, 1, "20"); // B3
        d.set_named_range("Rev", "B2:B3");
        d.set_cell_text(0, 0, "=SUM(Rev, 5)");
        assert_eq!(d.cell_display_value(0, 0), "35");
    }

    #[test]
    fn names_are_case_insensitive() {
        let mut d = DataProxy::new("t");
        d.set_cell_text(1, 1, "7"); // B2
        d.set_named_range("Foo", "B2");
        d.set_cell_text(0, 0, "=foo+1");
        assert_eq!(d.cell_display_value(0, 0), "8");
    }

    #[test]
    fn undefined_name_is_name_error() {
        let mut d = DataProxy::new("t");
        d.set_cell_text(0, 0, "=Nope+1");
        assert_eq!(d.cell_display_value(0, 0), "#NAME?");
        // …also when used as a function argument.
        d.set_cell_text(1, 0, "=SUM(Nope)");
        assert_eq!(d.cell_display_value(1, 0), "#NAME?");
    }

    // --- Fill handle (issue #12) ---

    #[test]
    fn shift_formula_refs_relative_and_anchored() {
        assert_eq!(shift_formula_refs("=A1+B1", 1, 0), "=A2+B2"); // down one row
        assert_eq!(shift_formula_refs("=A1+B1", 0, 1), "=B1+C1"); // right one col
        assert_eq!(shift_formula_refs("=$A$1+A1", 1, 0), "=$A$1+A2"); // anchored stays
        assert_eq!(shift_formula_refs("=A$1+$A1", 1, 1), "=B$1+$A2"); // mixed locks
    }

    #[test]
    fn fill_line_numeric_series() {
        assert_eq!(fill_line(&["1".into(), "2".into()], 3, true), vec!["3", "4", "5"]);
        assert_eq!(fill_line(&["2".into(), "4".into()], 2, true), vec!["6", "8"]);
        assert_eq!(fill_line(&["10".into(), "8".into()], 2, true), vec!["6", "4"]); // descending
    }

    #[test]
    fn fill_line_single_number_copies() {
        assert_eq!(fill_line(&["5".into()], 3, true), vec!["5", "5", "5"]);
    }

    #[test]
    fn fill_line_text_copies_cyclically() {
        assert_eq!(fill_line(&["a".into(), "b".into()], 3, true), vec!["a", "b", "a"]);
    }

    #[test]
    fn fill_line_formula_shifts() {
        // A single formula filled down shifts one extra row per step.
        assert_eq!(fill_line(&["=B1".into()], 3, true), vec!["=B2", "=B3", "=B4"]);
        // Filled right shifts columns instead.
        assert_eq!(fill_line(&["=A1".into()], 2, false), vec!["=B1", "=C1"]);
    }

    // --- Cross-sheet references (issue #4) ---

    /// Build a two-sheet workbook. `setup_other` populates the second sheet's
    /// cells with the given (row, col, value) triples. Returns the active
    /// (first) sheet, already wired to the registry.
    fn two_sheet_workbook(other: &[(usize, usize, &str)]) -> (DataProxy, SheetsRegistry) {
        let mut a = DataProxy::new("Sheet1");
        let mut b = DataProxy::new("Sheet2");
        for &(r, c, v) in other {
            b.set_cell_text(r, c, v);
        }
        let sheets: SheetsRegistry = Rc::new(RefCell::new(vec![a.clone(), b.clone()]));
        // Wire the registry on every DataProxy so the evaluator can find peers.
        for d in sheets.borrow_mut().iter_mut() {
            d.set_sheets(&sheets);
        }
        a.set_sheets(&sheets);
        // Hand back the strong Rc: each DataProxy now keeps only a Weak
        // back-reference (issue #4), so the caller must hold the registry alive.
        (a, sheets)
    }

    #[test]
    fn cross_sheet_cell_ref() {
        let (mut a, _reg) = two_sheet_workbook(&[(0, 0, "42")]); // Sheet2!A1
        a.set_cell_text(0, 0, "=Sheet2!A1");
        assert_eq!(a.cell_display_value(0, 0), "42");
    }

    #[test]
    fn cross_sheet_with_arithmetic() {
        let (mut a, _reg) = two_sheet_workbook(&[(0, 0, "10"), (1, 0, "20")]);
        a.set_cell_text(0, 0, "=Sheet2!A1+Sheet2!A2");
        assert_eq!(a.cell_display_value(0, 0), "30");
    }

    #[test]
    fn cross_sheet_range_in_function() {
        let (mut a, _reg) = two_sheet_workbook(&[(0, 0, "1"), (1, 0, "2"), (2, 0, "3"), (3, 0, "4")]);
        a.set_cell_text(0, 0, "=SUM(Sheet2!A1:A4)");
        assert_eq!(a.cell_display_value(0, 0), "10");
    }

    #[test]
    fn cross_sheet_unknown_sheet_is_ref_error() {
        let (mut a, _reg) = two_sheet_workbook(&[]);
        a.set_cell_text(0, 0, "=Missing!A1");
        assert_eq!(a.cell_display_value(0, 0), "#REF!");
        // And in a function arg position.
        a.set_cell_text(1, 0, "=SUM(Missing!A1:A2, 7)");
        assert_eq!(a.cell_display_value(1, 0), "#REF!");
    }

    #[test]
    fn cross_sheet_uses_target_sheet_value() {
        // A2 in Sheet2 is itself a formula; the cross-sheet ref sees the result.
        let (mut a, _reg) = two_sheet_workbook(&[(0, 0, "5"), (1, 0, "=Sheet2!A1*2")]);
        a.set_cell_text(0, 0, "=Sheet2!A2+1");
        assert_eq!(a.cell_display_value(0, 0), "11"); // 5*2 + 1
    }

    #[test]
    fn cross_sheet_name_is_case_insensitive() {
        let (mut a, _reg) = two_sheet_workbook(&[(0, 0, "7")]);
        a.set_cell_text(0, 0, "=sheet2!A1");
        assert_eq!(a.cell_display_value(0, 0), "7");
    }

    #[test]
    fn cross_sheet_cycle_terminates_without_overflow() {
        // Sheet1!A1 = Sheet2!A1 and Sheet2!A1 = Sheet1!A1 — a cross-sheet cycle.
        // The sheet-aware visited guard must break it instead of recursing
        // forever (issue #4). Evaluate the registry's own Sheet1 copy so both
        // sides see each other's live (cyclic) formula.
        let mut s1 = DataProxy::new("Sheet1");
        let mut s2 = DataProxy::new("Sheet2");
        s1.set_cell_text(0, 0, "=Sheet2!A1");
        s2.set_cell_text(0, 0, "=Sheet1!A1");
        let reg: SheetsRegistry = Rc::new(RefCell::new(vec![s1, s2]));
        for d in reg.borrow_mut().iter_mut() {
            d.set_sheets(&reg);
        }
        // Would stack-overflow (crash) before the fix; now the guard yields 0.
        let v = reg.borrow()[0].cell_display_value(0, 0);
        assert_eq!(v, "0");
    }

    #[test]
    fn registry_not_leaked_by_back_references() {
        // Each sheet's back-reference to the registry must be Weak, or the
        // Rc<Vec<DataProxy>> would form a cycle and leak the whole workbook.
        let reg: SheetsRegistry =
            Rc::new(RefCell::new(vec![DataProxy::new("S1"), DataProxy::new("S2")]));
        for d in reg.borrow_mut().iter_mut() {
            d.set_sheets(&reg);
        }
        let weak = Rc::downgrade(&reg);
        assert_eq!(Rc::strong_count(&reg), 1, "back-refs must be Weak, not strong Rc");
        drop(reg);
        assert!(weak.upgrade().is_none(), "workbook should free (no Rc cycle)");
    }

    #[test]
    fn no_registry_means_cross_sheet_is_ref_error() {
        // A DataProxy with no sheets registry should surface a #REF! rather
        // than panicking.
        let mut d = DataProxy::new("alone");
        d.set_cell_text(0, 0, "=Other!A1");
        assert_eq!(d.cell_display_value(0, 0), "#REF!");
    }

    #[test]
    fn adjust_formula_refs_preserves_sheet_prefix() {
        // The cell-ref substitution must not touch the `Sheet2!` prefix; the
        // *relative* cell ref still shifts like any other (issue #4).
        assert_eq!(adjust_formula_refs("=Sheet2!A1+B1", true, 0, 1, None), "=Sheet2!A2+B2");
        // Absolute row on the cross-sheet ref stays put.
        assert_eq!(adjust_formula_refs("=Sheet2!$A$1+A1", true, 0, 1, None), "=Sheet2!$A$1+A2");
        // Both ends of a cross-sheet range shift.
        assert_eq!(adjust_formula_refs("=Sheet2!A1:B3", true, 0, 1, None), "=Sheet2!A2:B4");
    }

    #[test]
    fn shift_formula_refs_preserves_sheet_prefix() {
        // The fill-handle shift must not corrupt cross-sheet refs.
        assert_eq!(shift_formula_refs("=Sheet2!A1", 1, 0), "=Sheet2!A2");
        assert_eq!(shift_formula_refs("=Sheet2!$A$1+A1", 0, 1), "=Sheet2!$A$1+B1");
        assert_eq!(shift_formula_refs("=Sheet2!A1:B3", 1, 0), "=Sheet2!A2:B4");
    }

    // --- Read-only mode & per-cell locking (issue #24) ---

    #[test]
    fn cells_default_to_editable() {
        // An unset cell uses Cell::default (editable=true), so the sheet
        // behaves like before the feature for callers that never opt in.
        let d = DataProxy::new("t");
        assert!(!d.is_read_only());
        assert!(d.is_cell_editable(0, 0));
    }

    #[test]
    fn read_only_blocks_every_cell() {
        let mut d = DataProxy::new("t");
        d.set_cell_text(0, 0, "hello");
        assert!(d.is_cell_editable(0, 0));
        d.set_read_only(true);
        assert!(d.is_read_only());
        // The previously-writable cell is now locked.
        assert!(!d.is_cell_editable(0, 0));
        // A brand-new cell is locked too.
        assert!(!d.is_cell_editable(5, 5));
    }

    #[test]
    fn read_only_set_cell_text_is_noop() {
        // Defense-in-depth: even if a caller forgets to check, the data
        // layer refuses the write (issue #24).
        let mut d = DataProxy::new("t");
        d.set_cell_text(0, 0, "before");
        d.set_read_only(true);
        d.set_cell_text(0, 0, "after");
        assert_eq!(d.get_cell_text(0, 0), "before");
    }

    #[test]
    fn locked_cell_blocks_set_cell_text() {
        let mut d = DataProxy::new("t");
        d.set_cell_text(0, 0, "before");
        d.set_cell_editable(0, 0, false);
        assert!(!d.is_cell_editable(0, 0));
        d.set_cell_text(0, 0, "after");
        assert_eq!(d.get_cell_text(0, 0), "before");
    }

    #[test]
    fn unlock_restores_editability() {
        let mut d = DataProxy::new("t");
        d.set_cell_text(0, 0, "before");
        d.set_cell_editable(0, 0, false);
        d.set_cell_text(0, 0, "blocked");
        assert_eq!(d.get_cell_text(0, 0), "before");
        d.set_cell_editable(0, 0, true);
        assert!(d.is_cell_editable(0, 0));
        d.set_cell_text(0, 0, "after");
        assert_eq!(d.get_cell_text(0, 0), "after");
    }

    #[test]
    fn read_only_takes_precedence_over_unlocked_cell() {
        // Even an explicitly-unlocked cell is locked while the sheet is
        // read-only.
        let mut d = DataProxy::new("t");
        d.set_cell_editable(0, 0, true);
        d.set_read_only(true);
        assert!(!d.is_cell_editable(0, 0));
    }

    // --- Text rotation / shrink-to-fit / indent (issue #25) ---

    #[test]
    fn style_defaults_have_no_rotation_or_indent() {
        // The new fields default to no-op so old saved data loads cleanly.
        let s = Style::default();
        assert_eq!(s.rotation, None);
        assert!(!s.shrink_to_fit);
        assert_eq!(s.indent, 0);
    }

    #[test]
    fn style_serde_round_trip_with_new_fields() {
        // Issue #25: the new fields must serialize and deserialize so
        // xlsx-style saved data stays loadable.
        let mut s = Style::default();
        s.rotation = Some(45.0);
        s.shrink_to_fit = true;
        s.indent = 17;
        let json = serde_json::to_string(&s).unwrap();
        let back: Style = serde_json::from_str(&json).unwrap();
        assert_eq!(back.rotation, Some(45.0));
        assert!(back.shrink_to_fit);
        assert_eq!(back.indent, 17);
    }

    #[test]
    fn style_serde_accepts_legacy_data_without_new_fields() {
        // Saved files from before #25 don't have the new fields. The
        // `#[serde(default)]` on each new field should fill them in.
        // Build the JSON with `format!` so we don't have to escape the
        // inner quote characters in a raw string.
        let legacy = format!(
            r#"{{"bgcolor":"{}","color":"{}","align":"left","valign":"middle","text_wrap":false,"underline":false,"strike":false,"bold":false,"italic":false,"font_size":10,"font_family":"Arial","format":"normal","border":null}}"#,
            "#ffffff", "#0a0a0a",
        );
        let s: Style = serde_json::from_str(&legacy).unwrap();
        assert_eq!(s.rotation, None);
        assert!(!s.shrink_to_fit);
        assert_eq!(s.indent, 0);
    }

    // --- Issue #9: data validation wiring ---

    use crate::core::validation::Validator;

    #[test]
    fn set_data_round_trips_validations() {
        // Bug: prior to #9, DataProxy::set_data did not read the
        // "validations" key, so any rule set on one sheet was lost on
        // reload. This test would have caught it.
        let mut d = DataProxy::new("t");
        d.validations
            .add("cell", "A1", Validator::new("list", false, "a,b,c", ""));
        d.validations
            .add("cell", "C3:E5", Validator::new("number", false, "1,10", "be"));
        let json = d.get_data_json();

        let mut d2 = DataProxy::new("t");
        d2.set_data_json(&json);
        // Both rules survive the round-trip.
        assert!(d2.validations.get(0, 0).is_some());
        assert!(d2.validations.get(4, 4).is_some());
        assert_eq!(d2.validations.get_data().len(), 2);
    }

    #[test]
    fn set_cell_text_records_validation_error() {
        // The data layer (DataProxy::set_cell_text) does not itself block
        // the write — that's the renderer's job. But the renderer's
        // chokepoint calls `Validations::validate` and that *does* populate
        // the errors map. We exercise that here.
        let mut d = DataProxy::new("t");
        d.validations
            .add("cell", "A1", Validator::new("list", false, "a,b", ""));
        // Manually invoke the validate hook (mirroring what the renderer
        // does inside set_cell_text_at) and confirm the error is recorded.
        assert!(!d.validations.validate(0, 0, "z"));
        assert!(d.validations.get_error(0, 0).is_some());
        // A valid value clears the error.
        assert!(d.validations.validate(0, 0, "a"));
        assert!(d.validations.get_error(0, 0).is_none());
    }

    #[test]
    fn required_validator_blocks_empty_value_at_validate_layer() {
        let mut d = DataProxy::new("t");
        d.validations
            .add("cell", "A1", Validator::new("text-length", true, "1,100", "be"));
        // Empty value is rejected because `required = true`.
        assert!(!d.validations.validate(0, 0, ""));
        assert!(d.validations.get_error(0, 0).is_some());
        // Whitespace-only is also rejected (the validator trims first).
        assert!(!d.validations.validate(0, 0, "   "));
        // Non-empty passes (it's also a valid number for the "be" operator).
        assert!(d.validations.validate(0, 0, "5"));
    }

    #[test]
    fn get_data_includes_validations_key() {
        let mut d = DataProxy::new("t");
        d.validations
            .add("cell", "A1", Validator::new("list", false, "a,b", ""));
        let json: serde_json::Value = serde_json::from_str(&d.get_data_json()).unwrap();
        assert!(json.get("validations").is_some(), "validations key must serialize");
        let arr = json.get("validations").unwrap().as_array().unwrap();
        assert_eq!(arr.len(), 1);
    }
}
