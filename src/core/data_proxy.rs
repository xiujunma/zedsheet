use crate::core::auto_filter::{AutoFilter, Sort};
use crate::core::cell::Cell;
use crate::core::cell_range::CellRange;
use crate::core::chart::Chart;
use crate::core::col::{Col, Cols};
use crate::core::cond_format::{lerp3_hex, lerp_hex, CondRule};
use crate::core::merges::Merges;
use crate::core::outline::OutlineGroup;
use crate::core::row::Row;
use crate::core::state::{Clipboard, History, Scroll, Selector};
use crate::core::table::Table;
use crate::core::validation::{Validation, Validations};
use crate::formula::parser::{tokenize, Token};
use crate::renderer::alphabets::{exp2xy, index_at, string_at};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::{Rc, Weak};

/// Shared registry of every sheet's `DataProxy`. Held by `ZedSheet` and
/// referenced by every `DataProxy` so cross-sheet formulas can resolve
/// `Sheet2!A1` against the right sheet (issue #4).
pub type SheetsRegistry = Rc<RefCell<Vec<DataProxy>>>;

/// Workbook-wide index of the currently-active sheet (issue #35). Held in a
/// `Rc<RefCell<…>>` so the renderer (which only sees a clone) and the
/// `ZedSheet` orchestrator (which owns the registry) can both update the
/// selection without a borrow check fight.
pub type ActiveSheet = Rc<RefCell<usize>>;

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

/// Render-time conditional-format visual for a cell (issue #29). Computed by
/// `DataProxy::cond_visual` and drawn by `render_cells` — these decorate the
/// cell rather than overriding its style.
#[derive(Debug, Clone, PartialEq)]
pub enum CondVisual {
    /// In-cell data bar: `frac` of the cell width (0..=1), in `color`.
    Bar { frac: f64, color: String },
    /// Icon at the cell's left edge; `zone` is 0 (low) / 1 (mid) / 2 (high).
    Icon { set: IconSet, zone: u8 },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IconSet {
    /// Down (red) / right (yellow) / up (green) arrows.
    Arrows,
    /// Red / yellow / green traffic lights.
    Traffic,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Border {
    pub left: Option<(String, String)>,
    pub right: Option<(String, String)>,
    pub top: Option<(String, String)>,
    pub bottom: Option<(String, String)>,
    /// Diagonal line from top-left to bottom-right of the cell.
    /// `None` means no diagonal. Same `(style, color)` tuple shape as
    /// the four edges so the existing JSON wire format stays a flat
    /// map. `skip_serializing_if` keeps pre-1.x workbooks identical on
    /// round-trip — a cell with only a top border serializes the same
    /// bytes before and after this change.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagonal_up: Option<(String, String)>,
    /// Diagonal line from bottom-left to top-right of the cell.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagonal_down: Option<(String, String)>,
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

/// Page setup for printing (issue #14). Stored per-sheet; round-trips
/// through `get_data` / `set_data`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageSetup {
    /// "portrait" or "landscape"
    #[serde(default = "default_orientation")]
    pub orientation: String,
    /// Paper size name: "letter", "a4", "legal", "a3"
    #[serde(default = "default_paper_size")]
    pub paper_size: String,
    /// Margins in inches: top, right, bottom, left
    #[serde(default = "default_margins")]
    pub margins: (f64, f64, f64, f64),
    /// Print scale as percentage (100 = no scaling)
    #[serde(default = "default_scale")]
    pub scale: u32,
    /// Optional print area as "A1:B4" or None for the used extent
    #[serde(default)]
    pub print_area: Option<String>,
    /// Row range to repeat at top of each page, e.g. "1:3"
    #[serde(default)]
    pub repeat_rows: Option<String>,
    /// Column range to repeat at left of each page, e.g. "A:B"
    #[serde(default)]
    pub repeat_cols: Option<String>,
    /// Manual page breaks (Phase 5.1). Each break is a tuple of
    /// (row_break, col_break) where exactly one is `Some` and the
    /// other is `None`: a row break ends the current page after
    /// that row, a col break ends the current page after that
    /// column. Sorted, deduped, and round-tripped through serde
    /// with `#[serde(default)]` so pre-1.5 workbooks load with an
    /// empty list.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub page_breaks: Vec<PageBreak>,
}

/// One manual page break (Phase 5.1). Stored as `(row, col)` with
/// exactly one side `Some` — a row break ends the current page
/// after that row; a col break ends the current page after that
/// column. The wrapper tuple (not two fields on PageSetup) keeps
/// the JSON a flat array of small structs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageBreak {
    pub row: Option<usize>,
    pub col: Option<usize>,
}

fn default_orientation() -> String {
    "portrait".into()
}
fn default_paper_size() -> String {
    "letter".into()
}
fn default_margins() -> (f64, f64, f64, f64) {
    (0.75, 0.75, 0.75, 0.75)
}
fn default_scale() -> u32 {
    100
}

impl Default for PageSetup {
    fn default() -> Self {
        Self {
            orientation: default_orientation(),
            paper_size: default_paper_size(),
            margins: default_margins(),
            scale: default_scale(),
            print_area: None,
            repeat_rows: None,
            repeat_cols: None,
            page_breaks: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct DataProxy {
    pub name: String,
    /// LET-name binding stack (Phase 3.2). Each LET invocation pushes
    /// a frame; the body span evaluates with the top frame visible.
    /// The frame is a `HashMap<String, Value>` so the same name in
    /// an outer scope isn't shadowed by the inner one — the inner
    /// shadow only affects lookups while the inner frame is on top.
    /// `Rc<RefCell<…>>` so the immutable evaluator API can still
    /// push / pop during a span's deferred evaluation.
    pub(crate) let_bindings: Rc<RefCell<Vec<HashMap<String, Value>>>>,
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
    /// Conditional-formatting rules, evaluated at render time (issue #11).
    pub cond_formats: Vec<CondRule>,
    /// Row outline groups (issue #30): collapsible ranges drawn as a gutter
    /// left of the row headers. Collapse state applies through the row hide
    /// flags, so hidden state itself also persists with the rows.
    pub row_groups: Vec<OutlineGroup>,
    /// Column outline groups (issue #30), drawn above the column headers.
    pub col_groups: Vec<OutlineGroup>,
    /// Print / page setup (issue #14). Per-sheet; round-trips through
    /// `get_data` / `set_data`.
    pub page_setup: PageSetup,
    /// Charts floating over the grid, anchored at cells (issue #16).
    pub charts: Vec<Chart>,
    /// Excel-style Tables (issue #34): named regions with a header row,
    /// banded data rows, an optional totals row, and structured-reference
    /// support in formulas.
    pub tables: Vec<Table>,
    /// Named ranges (sheet-scoped): UPPERCASE name → range expression like
    /// `"B2:B3"` or `"B2"`. Resolved by the evaluator and the name box.
    pub named_ranges: HashMap<String, String>,
    /// PivotTables defined on this sheet (issue #35). A pivot is the recipe
    /// (source range, row/col/value fields, aggregation, output sheet name);
    /// the materialized output lives on a separate `DataProxy` that's a
    /// sibling in the workbook's `SheetsRegistry`. This list survives
    /// workbook round-trip via `#[serde(default)]` so old workbooks load.
    pub pivots: Vec<crate::core::pivot::PivotTable>,
    /// Floating visual filters bound to this sheet's fields (issue #61).
    /// A slicer is a list of selected values for one column; the pivot
    /// engine reads it and applies it as an additional row predicate for
    /// every pivot whose spec references the bound field. Empty list =
    /// no slicing. Round-trips through `get_data` / `set_data` so
    /// pre-#61 workbooks (which lack this key) load with an empty vec.
    pub slicers: Vec<crate::core::pivot::Slicer>,
    /// Inline mini-charts (Phase 4.1). Each sparkline is anchored
    /// to a single cell and renders a tiny chart inside it. Read
    /// by the renderer after the body so they overlay on top of
    /// the cell's text. Empty list = no sparklines. Backward-compat:
    /// pre-4.1 workbooks don't include the key in `set_data`, so
    /// the field stays at its default (empty).
    pub sparklines: Vec<crate::core::sparkline::Sparkline>,
    /// Floating images anchored to single cells (Phase 4.2). Each
    /// entry has its own URL + anchor + size; the renderer fetches
    /// the URL once and blits the decoded image at the anchor.
    /// Empty list = no images. Backward-compat: pre-4.2 workbooks
    /// don't include the key in `set_data`, so the field stays
    /// at its default (empty).
    pub images: Vec<crate::core::image::Image>,
    /// Sheet protection metadata (Phase 1.3). The `enabled` flag mirrors
    /// `read_only` for the data-layer block — when protection is enabled
    /// the UI also sets `read_only = true`. `password_hash` is the
    /// optional djb2 hash that gates disabling. Backward-compat: pre-1.3
    /// workbooks don't include the `"protection"` key in `set_data`, so
    /// the field stays at its default.
    pub protection: crate::core::sheet_protection::SheetProtection,
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
    /// The cell whose formula is currently being evaluated, so position-aware
    /// functions (`ROW`/`COLUMN` with no argument) know their caller (issue
    /// #37). `eval_expr` sets it with save/restore, so a nested cell-reference
    /// evaluation restores the outer cell on return. Transient — not part of
    /// the serialized wire format.
    eval_cell: std::cell::Cell<(usize, usize)>,
    /// View zoom factor (issue #32), 0.1–4.0 (Excel's 10–400%). Applied in
    /// `get_row_height`/`get_col_width`, the single geometry source shared by
    /// the render path (Area/Viewport), hit-testing (`cell_at`/`track_at`),
    /// scrollbars, the editor overlay, and chart anchors — so screen geometry
    /// stays consistent everywhere. Stored row heights / column widths remain
    /// in unzoomed model pixels. Transient — not serialized; print stamps 1.0.
    zoom: f64,
    /// Computed dynamic-array spills (issue #33). A pure function of the
    /// sheet's cells, rebuilt lazily when `spills_dirty` — so a `clone()`
    /// (undo snapshots, `find_sheet`) carries a cache consistent with its
    /// own cell snapshot. Transient — not part of the serialized wire format.
    spills: RefCell<SpillCache>,
    /// Set by every value-changing mutation; cleared by `ensure_spills`.
    spills_dirty: std::cell::Cell<bool>,
    /// Re-entrancy guard: evaluating anchors during a rebuild must not
    /// trigger a nested rebuild.
    spills_computing: std::cell::Cell<bool>,
}

/// Where dynamic-array results landed (issue #33): which cells are spill
/// anchors (and whether their spill fit), and the value shown in every cell a
/// successful spill covers.
#[derive(Debug, Clone, Default)]
struct SpillCache {
    /// Anchor → `Some((rows, cols))` when the array spilled, `None` when the
    /// target range was obstructed or out of bounds (the anchor shows #SPILL!).
    anchors: HashMap<(usize, usize), Option<(usize, usize)>>,
    /// Displayed value for every cell covered by a successful spill,
    /// including the anchor itself (so volatile RANDARRAY anchors render the
    /// same numbers as their spilled cells).
    values: HashMap<(usize, usize), Value>,
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
            cond_formats: Vec::new(),
            row_groups: Vec::new(),
            col_groups: Vec::new(),
            page_setup: PageSetup::default(),
            charts: Vec::new(),
            tables: Vec::new(),
            named_ranges: HashMap::new(),
            let_bindings: Rc::new(RefCell::new(Vec::new())),
            pivots: Vec::new(),
            slicers: Vec::new(),
            sparklines: Vec::new(),
            images: Vec::new(),
            protection: crate::core::sheet_protection::SheetProtection::default(),
            sheets: None,
            read_only: Rc::new(RefCell::new(false)),
            eval_cell: std::cell::Cell::new((0, 0)),
            zoom: 1.0,
            spills: RefCell::new(SpillCache::default()),
            spills_dirty: std::cell::Cell::new(true),
            spills_computing: std::cell::Cell::new(false),
        }
    }
}

impl DataProxy {
    pub fn new(name: &str) -> Self {
        DataProxy {
            name: name.to_string(),
            ..Default::default()
        }
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

    /// Enable / disable sheet protection (Phase 1.3). Toggling on
    /// also flips `read_only` so the data-layer "block edits" guard
    /// stays in sync; toggling off clears both. The optional
    /// `password` is hashed via [`SheetProtection::hash_password`];
    /// `None` or `""` clears any existing password (an empty
    /// password is treated as "no password").
    pub fn set_protection(&mut self, enabled: bool, password: Option<&str>) {
        self.protection.enabled = enabled;
        self.protection.password_hash = match password {
            Some(p) if !p.is_empty() => {
                Some(crate::core::sheet_protection::SheetProtection::hash_password(p))
            }
            _ => None,
        };
        // Mirror the enable flag on the data-layer lock so the
        // existing `is_cell_editable` guard (and `setSheetReadOnly`
        // JS callers) see the same state.
        self.set_read_only(enabled);
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
        let row = self.rows.entry(ri).or_default();
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
        self.mark_spills_dirty();
        let cell = self.get_cell_or_new(ri, ci);
        // Phase 4.5b inline-edit: when the user types into the
        // formula bar of a rich-text cell, decide whether to keep
        // the per-run styling. If the new text matches the
        // joined run text (case-insensitive, trim), the run
        // structure is still meaningful. Otherwise, drop runs
        // and re-render as plain.
        if let Some(runs) = cell.runs.as_ref() {
            let joined: String = runs.iter().map(|r| r.text.as_str()).collect();
            if !joined.eq_ignore_ascii_case(text.trim()) {
                cell.runs = None;
            }
        }
        cell.set_text(text);
    }

    pub fn get_cell_text(&self, ri: usize, ci: usize) -> String {
        self.get_cell(ri, ci)
            .map(|c| c.text.clone())
            .unwrap_or_default()
    }

    /// The text shown for a cell: formulas (text starting with `=`) are
    /// evaluated; everything else is returned verbatim.
    pub fn cell_display_value(&self, ri: usize, ci: usize) -> String {
        let raw = self.cell_raw_value(ri, ci);
        // Apply the cell's display format (number/currency/percent/…).
        let fmt = self.get_cell_style(ri, ci).format;
        crate::core::format::format_value(&raw, &fmt)
    }

    /// The cell's computed value BEFORE display formatting — formulas
    /// evaluated, but no currency/percent decoration. Conditional-format
    /// rules match against this so `> 150` works on a `$`-formatted column
    /// (issue #11).
    pub fn cell_raw_value(&self, ri: usize, ci: usize) -> String {
        let text = self.get_cell_text(ri, ci);
        // A cell that literally holds an error value displays it verbatim.
        if EvalErr::from_literal(&text).is_some() {
            return text;
        }
        if let Some(expr) = text.strip_prefix('=') {
            // A spill anchor displays its cached top-left value — or #SPILL!
            // when the spill range is obstructed (issue #33).
            self.ensure_spills();
            match self.spills.borrow().anchors.get(&(ri, ci)) {
                Some(None) => return EvalErr::Spill.code().to_string(),
                Some(Some(_)) => {
                    if let Some(v) = self.spills.borrow().values.get(&(ri, ci)) {
                        return value_display(v);
                    }
                }
                None => {}
            }
            let mut visited: Visited = HashSet::new();
            visited.insert((self.name.clone(), ri, ci));
            match self.eval_expr(expr, (ri, ci), &mut visited) {
                Ok(v) => value_display(&v),
                Err(e) => e.code().to_string(),
            }
        } else if text.is_empty() {
            // An empty cell covered by a spill shows the spilled value
            // (issue #33).
            self.ensure_spills();
            match self.spills.borrow().values.get(&(ri, ci)) {
                Some(v) => value_display(v),
                None => text,
            }
        } else {
            text
        }
    }

    // --- Dynamic-array spill (issue #33) ---

    /// Flag the spill cache stale. Called by every mutation that can change a
    /// cell's value or a formula's meaning; cheap — the rebuild is lazy.
    pub fn mark_spills_dirty(&self) {
        self.spills_dirty.set(true);
    }

    /// Successful multi-cell spill ranges, for the renderer's outline.
    /// Rebuilds the cache first if it is stale.
    pub fn spill_ranges(&self) -> Vec<CellRange> {
        self.ensure_spills();
        let cache = self.spills.borrow();
        let mut out: Vec<CellRange> = cache
            .anchors
            .iter()
            .filter_map(|(&(r, c), sz)| match sz {
                Some((rs, cs)) if rs * cs > 1 => Some(CellRange::new(r, c, r + rs - 1, c + cs - 1)),
                _ => None,
            })
            .collect();
        out.sort_by_key(|r| (r.sri, r.sci));
        out
    }

    /// Rebuild the spill cache if stale. Anchors are processed in row-major
    /// order, so a spill can feed a later one; the `spills_computing` guard
    /// keeps the evaluation those rebuilds trigger from recursing back here.
    fn ensure_spills(&self) {
        if self.spills_computing.get() || !self.spills_dirty.get() {
            return;
        }
        self.spills_computing.set(true);
        {
            let mut cache = self.spills.borrow_mut();
            cache.anchors.clear();
            cache.values.clear();
        }
        // Candidate anchors: formulas mentioning an array function. Only
        // these can evaluate to a `Value::Array` at top level.
        let mut anchors: Vec<(usize, usize)> = Vec::new();
        for (&ri, row) in &self.rows {
            for (&ci, cell) in &row.cells {
                if cell.text.starts_with('=') && is_spill_candidate(&cell.text) {
                    anchors.push((ri, ci));
                }
            }
        }
        anchors.sort_unstable();
        for (ri, ci) in anchors {
            let text = self.get_cell_text(ri, ci);
            let Some(expr) = text.strip_prefix('=') else {
                continue;
            };
            let mut visited: Visited = HashSet::new();
            visited.insert((self.name.clone(), ri, ci));
            // Scalar results and errors take the ordinary display path; only
            // a genuine array result is a spill anchor.
            let Ok(Value::Array(grid)) = self.eval_expr(expr, (ri, ci), &mut visited) else {
                continue;
            };
            let rows = grid.len();
            let cols = grid.iter().map(Vec::len).max().unwrap_or(0);
            if rows == 0 || cols == 0 {
                continue;
            }
            let range = CellRange::new(ri, ci, ri + rows - 1, ci + cols - 1);
            // The grid edge blocks a spill only when crossed from inside —
            // the cell model itself is sparse and tolerates out-of-bounds
            // anchors (tests park helper cells there).
            let blocked = (ri < self.row_count && ri + rows > self.row_count)
                || (ci < self.cols.len && ci + cols > self.cols.len)
                || self.merges.intersects(&range)
                || {
                    let cache = self.spills.borrow();
                    (ri..ri + rows).any(|r| {
                        (ci..ci + cols).any(|c| {
                            (r, c) != (ri, ci)
                                && (!self.get_cell_text(r, c).is_empty()
                                    || cache.values.contains_key(&(r, c))
                                    || cache.anchors.contains_key(&(r, c)))
                        })
                    })
                };
            let mut cache = self.spills.borrow_mut();
            if blocked {
                cache.anchors.insert((ri, ci), None);
            } else {
                cache.anchors.insert((ri, ci), Some((rows, cols)));
                for (dr, row_vals) in grid.iter().enumerate() {
                    for dc in 0..cols {
                        // Pad a ragged row with blanks to keep the spill
                        // rectangular.
                        let v = row_vals.get(dc).cloned().unwrap_or(Value::Blank);
                        cache.values.insert((ri + dr, ci + dc), v);
                    }
                }
            }
        }
        self.spills_dirty.set(false);
        self.spills_computing.set(false);
    }

    /// Overlay the first matching conditional-format rule onto `style`
    /// (issue #11). Comparison/contains rules apply their fixed overrides;
    /// a `scale2` rule interpolates the fill between its two colors across
    /// the range's numeric values.
    pub fn apply_cond_format(&self, ri: usize, ci: usize, style: &mut Style) {
        for rule in &self.cond_formats {
            let Some((r0, c0, r1, c1)) = rule.bounds() else {
                continue;
            };
            if ri < r0 || ri > r1 || ci < c0 || ci > c1 {
                continue;
            }
            match rule.op.as_str() {
                // Color scales compute a fill from the cell's position in the
                // range's numeric span (issue #11 / #29).
                "scale2" | "scale3" => {
                    let Ok(n) = self.cell_raw_value(ri, ci).trim().parse::<f64>() else {
                        continue;
                    };
                    let Some((min, max)) = self.cond_range_min_max((r0, c0, r1, c1)) else {
                        continue;
                    };
                    let t = if max > min {
                        (n - min) / (max - min)
                    } else {
                        0.5
                    };
                    let bg = if rule.op == "scale2" {
                        lerp_hex(&rule.v1, &rule.v2, t)
                    } else {
                        lerp3_hex(&rule.v1, &rule.v2, &rule.v3, t)
                    };
                    if let Some(bg) = bg {
                        style.bgcolor = Some(bg);
                        return;
                    }
                }
                // Render-time visuals (issue #29): no style override here —
                // the render path asks `cond_visual` for these.
                "databar" | "icons" => continue,
                _ => {
                    if self.cond_rule_matches(rule, ri, ci, (r0, c0, r1, c1)) {
                        if let Some(bg) = &rule.bgcolor {
                            style.bgcolor = Some(bg.clone());
                        }
                        if let Some(c) = &rule.color {
                            style.color = c.clone();
                        }
                        if rule.bold {
                            style.bold = true;
                        }
                        return; // first matching style rule wins
                    }
                }
            }
        }
    }

    /// Numeric min/max across a conditional-format rule's range, or `None`
    /// when the range holds no numbers. Shared by the color scales, data
    /// bars, and icon sets (issue #29).
    fn cond_range_min_max(&self, b: (usize, usize, usize, usize)) -> Option<(f64, f64)> {
        let (r0, c0, r1, c1) = b;
        let (mut min, mut max) = (f64::INFINITY, f64::NEG_INFINITY);
        for r in r0..=r1 {
            for c in c0..=c1 {
                if let Ok(x) = self.cell_raw_value(r, c).trim().parse::<f64>() {
                    min = min.min(x);
                    max = max.max(x);
                }
            }
        }
        (min.is_finite() && max.is_finite()).then_some((min, max))
    }

    /// All numeric values in a rule's range, in scan order.
    fn cond_range_numbers(&self, b: (usize, usize, usize, usize)) -> Vec<f64> {
        let (r0, c0, r1, c1) = b;
        let mut out = Vec::new();
        for r in r0..=r1 {
            for c in c0..=c1 {
                if let Ok(x) = self.cell_raw_value(r, c).trim().parse::<f64>() {
                    out.push(x);
                }
            }
        }
        out
    }

    /// Whether a boolean conditional-format rule matches cell (ri, ci). Pure
    /// value comparisons delegate to `CondRule::matches_value`; the rule types
    /// added by issue #29 (top/bottom-N, above/below average, duplicate/unique,
    /// formula) need range-wide or sheet context, so they are resolved here.
    fn cond_rule_matches(
        &self,
        rule: &CondRule,
        ri: usize,
        ci: usize,
        bounds: (usize, usize, usize, usize),
    ) -> bool {
        let raw = self.cell_raw_value(ri, ci);
        match rule.op.as_str() {
            "top" | "bottom" => {
                let Ok(n) = raw.trim().parse::<f64>() else {
                    return false;
                };
                let mut vals = self.cond_range_numbers(bounds);
                if vals.is_empty() {
                    return false;
                }
                // `v1` is a count ("3") or a percentage ("10%").
                let v1 = rule.v1.trim();
                let count = if let Some(pct) = v1.strip_suffix('%') {
                    let Ok(p) = pct.trim().parse::<f64>() else {
                        return false;
                    };
                    ((vals.len() as f64 * p / 100.0).round() as usize).max(1)
                } else {
                    let Ok(k) = v1.parse::<usize>() else {
                        return false;
                    };
                    k
                };
                let count = count.min(vals.len());
                if count == 0 {
                    return false;
                }
                if rule.op == "top" {
                    vals.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
                    n >= vals[count - 1] // threshold comparison includes ties
                } else {
                    vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                    n <= vals[count - 1]
                }
            }
            "above-avg" | "below-avg" => {
                let Ok(n) = raw.trim().parse::<f64>() else {
                    return false;
                };
                let vals = self.cond_range_numbers(bounds);
                if vals.is_empty() {
                    return false;
                }
                let avg = vals.iter().sum::<f64>() / vals.len() as f64;
                // Excel's default rules are strict comparisons to the mean.
                if rule.op == "above-avg" {
                    n > avg
                } else {
                    n < avg
                }
            }
            "dup" | "unique" => {
                let needle = raw.trim().to_lowercase();
                if needle.is_empty() {
                    return false; // blanks are neither duplicates nor unique
                }
                let (r0, c0, r1, c1) = bounds;
                let mut count = 0usize;
                for r in r0..=r1 {
                    for c in c0..=c1 {
                        if self.cell_raw_value(r, c).trim().to_lowercase() == needle {
                            count += 1;
                        }
                    }
                }
                if rule.op == "dup" {
                    count >= 2
                } else {
                    count == 1
                }
            }
            "formula" => {
                // Excel semantics: the formula is written for the range's
                // top-left cell and its RELATIVE references shift with each
                // cell ($-anchored parts stay put) — the same shift used when
                // pasting formulas.
                let expr = rule.v1.trim().trim_start_matches('=');
                if expr.is_empty() {
                    return false;
                }
                let (r0, c0, _, _) = bounds;
                let shifted =
                    shift_formula_refs(expr, ri as isize - r0 as isize, ci as isize - c0 as isize);
                let mut visited: Visited = HashSet::new();
                matches!(self.eval_expr(&shifted, (ri, ci), &mut visited), Ok(v) if v.is_truthy())
            }
            _ => rule.matches_value(&raw),
        }
    }

    /// The render-time conditional-format visuals for a cell (issue #29
    /// follow-on): a list of in-cell data bars + icons. Returns ALL
    /// matching visuals so a bar AND an icon rule can stack on the
    /// same cell — the renderer iterates the list and draws bars
    /// first, then icons on top. `cond_visual` is the single-result
    /// convenience for callers that only need the first match.
    pub fn cond_visuals(&self, ri: usize, ci: usize) -> Vec<CondVisual> {
        let mut out = Vec::new();
        for rule in &self.cond_formats {
            if rule.op != "databar" && rule.op != "icons" {
                continue;
            }
            let Some((r0, c0, r1, c1)) = rule.bounds() else {
                continue;
            };
            if ri < r0 || ri > r1 || ci < c0 || ci > c1 {
                continue;
            }
            let Ok(n) = self.cell_raw_value(ri, ci).trim().parse::<f64>() else {
                continue;
            };
            let Some((min, max)) = self.cond_range_min_max((r0, c0, r1, c1)) else {
                continue;
            };
            let t = if max > min {
                (n - min) / (max - min)
            } else {
                1.0
            };
            if rule.op == "databar" {
                // Keep the range minimum visible as a sliver, like Excel.
                let frac = 0.05 + 0.95 * t.clamp(0.0, 1.0);
                let color = rule
                    .bgcolor
                    .clone()
                    .filter(|c| !c.is_empty())
                    .unwrap_or_else(|| "#638ec6".to_string()); // Excel's default blue
                out.push(CondVisual::Bar { frac, color });
            } else {
                // Icon zone by thirds of the range's numeric span.
                let zone = if t < 1.0 / 3.0 {
                    0
                } else if t < 2.0 / 3.0 {
                    1
                } else {
                    2
                };
                let set = if rule.v1.trim().eq_ignore_ascii_case("traffic") {
                    IconSet::Traffic
                } else {
                    IconSet::Arrows
                };
                out.push(CondVisual::Icon { set, zone });
            }
        }
        out
    }

    /// Convenience: the first cond-format visual for a cell, or
    /// `None`. Kept for callers that only need a single visual.
    pub fn cond_visual(&self, ri: usize, ci: usize) -> Option<CondVisual> {
        self.cond_visuals(ri, ci).into_iter().next()
    }

    /// Resolve a cell to a formula value (issue #2). Error values propagate;
    /// numeric text becomes a number and date text its serial (so `=A1+1`
    /// works); other text stays text so string functions and comparisons see
    /// it. Blank cells resolve to 0, matching the engine's historic numeric
    /// behavior. Nested formulas recurse with a circular-ref guard.
    fn resolve_value(&self, ri: usize, ci: usize, visited: &mut Visited) -> Result<Value, EvalErr> {
        // Key by sheet so the same (row, col) on different sheets don't collide
        // and a cross-sheet cycle is detected (issue #4).
        let key = (self.name.clone(), ri, ci);
        if visited.contains(&key) {
            return Ok(Value::Number(0.0));
        }
        let text = self.get_cell_text(ri, ci);
        if let Some(e) = EvalErr::from_literal(&text) {
            return Err(e);
        }
        if let Some(expr) = text.strip_prefix('=') {
            // A spill anchor resolves to its cached top-left value (a
            // reference to the anchor sees one cell, like Excel without the
            // `A1#` spilled-range operator); a blocked anchor propagates
            // #SPILL! (issue #33).
            self.ensure_spills();
            match self.spills.borrow().anchors.get(&(ri, ci)) {
                Some(None) => return Err(EvalErr::Spill),
                Some(Some(_)) => {
                    if let Some(v) = self.spills.borrow().values.get(&(ri, ci)) {
                        return Ok(v.clone());
                    }
                }
                None => {}
            }
            visited.insert(key.clone());
            let v = self.eval_expr(expr, (ri, ci), visited);
            visited.remove(&key);
            v
        } else {
            let t = text.trim();
            if t.is_empty() {
                // An empty cell covered by a spill resolves to the spilled
                // value, so `=B2` and ranges over a spill see real values
                // (issue #33).
                self.ensure_spills();
                if let Some(v) = self.spills.borrow().values.get(&(ri, ci)) {
                    return Ok(v.clone());
                }
                Ok(Value::Blank)
            } else if let Ok(n) = t.parse::<f64>() {
                Ok(Value::Number(n))
            } else if let Some(serial) = crate::core::date::parse_date(t) {
                Ok(Value::Number(serial))
            } else {
                Ok(Value::Text(text))
            }
        }
    }

    fn eval_expr(
        &self,
        expr: &str,
        cell: (usize, usize),
        visited: &mut Visited,
    ) -> Result<Value, EvalErr> {
        // Track the calling cell for position-aware functions (ROW/COLUMN),
        // save/restore so a nested cell-ref eval restores the outer cell on
        // return — `=ROW() + B1 + ROW()` keeps both ROW()s reading this cell
        // even though resolving B1 re-enters eval_expr (issue #37).
        let prev = self.eval_cell.get();
        self.eval_cell.set(cell);
        let tokens = tokenize(expr);
        let mut pos = 0usize;
        let v = self.parse_cmp(&tokens, &mut pos, visited);
        self.eval_cell.set(prev);
        v
    }

    // expr := add ((= | == | <> | > | < | >= | <=) add)* — comparisons yield 1/0.
    // Numbers compare numerically, text case-insensitively; a number never
    // equals text (and orders before it), matching Excel. With an array
    // operand the comparison broadcasts element-wise (issue #33), which is
    // what makes `FILTER(A1:A9, B1:B9>5)` work.
    fn parse_cmp(&self, t: &[Token], pos: &mut usize, vis: &mut Visited) -> Result<Value, EvalErr> {
        let mut v = self.parse_add(t, pos, vis)?;
        while *pos < t.len() {
            if let Token::Operator(op) = &t[*pos] {
                if matches!(op.as_str(), "=" | "==" | "<>" | ">" | "<" | ">=" | "<=") {
                    let op = op.clone();
                    *pos += 1;
                    let r = self.parse_add(t, pos, vis)?;
                    v = broadcast2(&v, &r, &|x, y| {
                        use std::cmp::Ordering::*;
                        let ord = compare_values(x, y);
                        let b = match op.as_str() {
                            "=" | "==" => ord == Some(Equal),
                            "<>" => ord != Some(Equal),
                            ">" => ord == Some(Greater),
                            "<" => ord == Some(Less),
                            ">=" => matches!(ord, Some(Greater | Equal)),
                            "<=" => matches!(ord, Some(Less | Equal)),
                            _ => false,
                        };
                        Ok(Value::Number(if b { 1.0 } else { 0.0 }))
                    })?;
                    continue;
                }
            }
            break;
        }
        Ok(v)
    }

    // expr := term (('+' | '-') term)* — arithmetic coerces text to numbers
    // and broadcasts element-wise over array operands (issue #33).
    fn parse_add(&self, t: &[Token], pos: &mut usize, vis: &mut Visited) -> Result<Value, EvalErr> {
        let mut v = self.parse_mul(t, pos, vis)?;
        while *pos < t.len() {
            if let Token::Operator(op) = &t[*pos] {
                if op == "+" || op == "-" {
                    let plus = op == "+";
                    *pos += 1;
                    let r = self.parse_mul(t, pos, vis)?;
                    v = broadcast2(&v, &r, &|x, y| {
                        let (l, r) = (x.as_number(), y.as_number());
                        Ok(Value::Number(if plus { l + r } else { l - r }))
                    })?;
                    continue;
                }
            }
            break;
        }
        Ok(v)
    }

    // term := factor (('*' | '/') factor)*
    fn parse_mul(&self, t: &[Token], pos: &mut usize, vis: &mut Visited) -> Result<Value, EvalErr> {
        let mut v = self.parse_factor(t, pos, vis)?;
        while *pos < t.len() {
            if let Token::Operator(op) = &t[*pos] {
                if op == "*" || op == "/" {
                    let mul = op == "*";
                    *pos += 1;
                    let r = self.parse_factor(t, pos, vis)?;
                    v = broadcast2(&v, &r, &|x, y| {
                        let (l, r) = (x.as_number(), y.as_number());
                        if mul {
                            Ok(Value::Number(l * r))
                        } else if r == 0.0 {
                            Err(EvalErr::Div0)
                        } else {
                            Ok(Value::Number(l / r))
                        }
                    })?;
                    continue;
                }
            }
            break;
        }
        Ok(v)
    }

    // factor := Number | '-' factor | '(' expr ')' | Function '(' args ')' | CellRef
    fn parse_factor(
        &self,
        t: &[Token],
        pos: &mut usize,
        vis: &mut Visited,
    ) -> Result<Value, EvalErr> {
        let tok = t.get(*pos).ok_or(EvalErr::Value)?.clone();
        eprintln!("DBG parse_factor tok={:?}", tok);
        match tok {
            Token::Number(n) => {
                *pos += 1;
                Ok(Value::Number(n))
            }
            Token::Error(code) => {
                *pos += 1;
                Err(EvalErr::from_literal(&code).unwrap_or(EvalErr::Value))
            }
            Token::Operator(op) if op == "-" => {
                *pos += 1;
                let v = self.parse_factor(t, pos, vis)?;
                // Negation broadcasts over an array operand (issue #33).
                broadcast2(&v, &Value::Blank, &|x, _| Ok(Value::Number(-x.as_number())))
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
                // IF/IFS/CHOOSE are short-circuit (issue #38): capture each
                // argument's token span without evaluating, then evaluate only
                // the condition and the chosen branch. An error in a not-taken
                // branch (e.g. `=IF(TRUE(), 1, 1/0)`) must not propagate.
                let upper = name.to_uppercase();
                if matches!(upper.as_str(), "IF" | "IFS" | "CHOOSE") {
                    let spans = self.arg_spans(t, pos);
                    return self.eval_lazy(&upper, t, &spans, vis);
                }
                // LET (Phase 3.2): name-binding scope over a body span.
                // Spans-based evaluation lets the body see the LET
                // frame without leaking the bindings to the outer scope.
                if upper == "LET" {
                    let spans = self.arg_spans(t, pos);
                    return self.eval_let(t, &spans, vis);
                }
                // LAMBDA (Phase 3.3): capture params + body tokens and
                // return a `Value::Lambda`. The body doesn't evaluate
                // here — MAP (or REDUCE, BYROW, …) invokes it.
                if upper == "LAMBDA" {
                    let spans = self.arg_spans(t, pos);
                    return self.eval_lambda(t, spans);
                }
                // MAP (Phase 3.3): apply a LAMBDA element-wise to an
                // array. Spans-based dispatch because the lambda
                // arg must NOT be eagerly evaluated (it isn't a
                // primitive Value).
                if upper == "MAP" {
                    let spans = self.arg_spans(t, pos);
                    return self.eval_map(t, &spans, vis);
                }
                // REDUCE / BYROW / BYCOL / MAKEARRAY (Phase 3.4):
                // every one reuses `call_lambda` so the lambda body
                // sees the LET-binding frame exactly the way MAP does.
                if upper == "REDUCE" {
                    let spans = self.arg_spans(t, pos);
                    return self.eval_reduce(t, &spans, vis);
                }
                if upper == "BYROW" {
                    let spans = self.arg_spans(t, pos);
                    return self.eval_byrow(t, &spans, vis);
                }
                if upper == "BYCOL" {
                    let spans = self.arg_spans(t, pos);
                    return self.eval_bycol(t, &spans, vis);
                }
                if upper == "MAKEARRAY" {
                    let spans = self.arg_spans(t, pos);
                    return self.eval_makearray(t, &spans, vis);
                }
                // Position / reference functions (issue #37) need the calling
                // cell or their argument's coordinates, not its value — they
                // read the raw arg spans. SUBTOTAL (issue #30) joins them
                // because it must see each value's ROW to honor hidden rows.
                // In scalar context a returned range collapses to its top-left.
                if matches!(
                    upper.as_str(),
                    "ROW" | "COLUMN" | "OFFSET" | "INDIRECT" | "ADDRESS" | "SUBTOTAL"
                ) {
                    let spans = self.arg_spans(t, pos);
                    return self
                        .eval_ref_fn(&upper, t, &spans, vis)
                        .map(|a| a.to_scalar());
                }
                let args = self.parse_args(t, pos, vis);
                // CELL needs self to access the sheet name and position.
                if name.eq_ignore_ascii_case("CELL") {
                    return self.eval_cell_fn(&args);
                }
                // Functions that must observe a *failed* or typed argument
                // (IFERROR, IFNA, IS*) are resolved here, where the per-argument
                // results — including evaluation errors — are still visible
                // (issue #2/#27).
                if let Some(res) = apply_info_function(&name, &args) {
                    return res;
                }
                // Every other function propagates the first failing argument,
                // preserving the engine's historic error behavior.
                let mut ok_args = Vec::with_capacity(args.len());
                for a in args {
                    ok_args.push(a?);
                }
                apply_function(&name, &ok_args)
            }
            Token::CellRef(r) => {
                // A range in expression position (`A1:A5>2`, top-level
                // `=A1:B3`) resolves to an array so operators broadcast over
                // it and the result can spill (issue #33). Bare ranges in
                // argument position never reach here — `parse_args` keeps
                // them as `Arg::Range` directly.
                if let (Some(Token::Colon), Some(Token::CellRef(b))) =
                    (t.get(*pos + 1), t.get(*pos + 2))
                {
                    let b = b.clone();
                    *pos += 3;
                    let (c0, r0) = exp2xy(&r);
                    let (c1, r1) = exp2xy(&b);
                    let arg =
                        self.resolve_grid(r0.min(r1), c0.min(c1), r0.max(r1), c0.max(c1), vis)?;
                    return Ok(Value::Array(arg.grid()));
                }
                *pos += 1;
                let (c, row) = exp2xy(&r);
                self.resolve_value(row, c, vis)
            }
            Token::SheetRange { sheet, from, to } => {
                *pos += 1;
                // Cross-sheet range in expression position (issue #33).
                let Some(target) = self.find_sheet(&sheet) else {
                    return Err(EvalErr::Ref);
                };
                let (c0, r0) = exp2xy(&from);
                let (c1, r1) = exp2xy(&to);
                let arg =
                    target.resolve_grid(r0.min(r1), c0.min(c1), r0.max(r1), c0.max(c1), vis)?;
                Ok(Value::Array(arg.grid()))
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
                    target.resolve_value(row, c, vis)
                } else {
                    Err(EvalErr::Ref)
                }
            }
            Token::Name(n) => {
                eprintln!("DBG name arm n={:?}, pos={}, t.len={}", n, *pos, t.len());
                *pos += 1;
                // Phase 3.2: a LET binding takes precedence over a
                // sheet-scoped named range, but only while its frame
                // Walk top → bottom so an inner LET/LAMBDA can
                // shadow an outer name while a LAMBDA body still
                // sees LET bindings from the enclosing scope that
                // aren't shadowed by its own params.
                for frame in self.let_bindings.borrow().iter().rev() {
                    if let Some(v) = frame.get(&n) {
                        return Ok(v.clone());
                    }
                }
                // Scalar context: a named range resolves to its top-left cell;
                // an undefined name is a #NAME? error.
                match self.resolve_name(&n) {
                    Some((r0, c0, _, _)) => self.resolve_value(r0, c0, vis),
                    None => Err(EvalErr::Name),
                }
            }
            Token::StructRef { table, spec } => {
                *pos += 1;
                // Structured table reference (issue #34): `[@Col]` yields the
                // intersecting cell; everything else yields an array, which
                // broadcasts in expressions and spills at top level.
                match self.resolve_struct_ref(&table, &spec, vis)? {
                    Arg::Scalar(v) => Ok(v),
                    Arg::Range(rows) => Ok(Value::Array(rows)),
                }
            }
            Token::String(s) => {
                *pos += 1;
                Ok(Value::Text(s))
            }
            Token::LeftBrace => {
                // Inline array literal (Phase 3.1):
                //   {1, 2, 3; 4, 5, 6}
                // Semicolons separate rows, commas separate elements.
                // Each element is a full expression so we re-use
                // `parse_cmp` for every cell.
                *pos += 1;
                let mut rows: Vec<Vec<Value>> = Vec::new();
                let mut current_row: Vec<Value> = Vec::new();
                if matches!(t.get(*pos), Some(Token::RightBrace)) {
                    return Err(EvalErr::Value);
                }
                loop {
                    let v = self.parse_cmp(t, pos, vis)?;
                    current_row.push(v);
                    match t.get(*pos) {
                        Some(Token::Comma) => {
                            *pos += 1;
                            continue;
                        }
                        Some(Token::Semicolon) => {
                            *pos += 1;
                            rows.push(std::mem::take(&mut current_row));
                            continue;
                        }
                        Some(Token::RightBrace) => {
                            *pos += 1;
                            rows.push(std::mem::take(&mut current_row));
                            break;
                        }
                        _ => return Err(EvalErr::Value),
                    }
                }
                if rows.is_empty() {
                    return Err(EvalErr::Value);
                }
                // Validate rectangular shape (all rows the same length).
                let width = rows[0].len();
                if width == 0 || rows.iter().any(|r| r.len() != width) {
                    return Err(EvalErr::Value);
                }
                Ok(Value::Array(rows))
            }
            _ => Err(EvalErr::Value),
        }
    }

    /// Resolve a rectangular block to a row-major grid of values.
    fn resolve_grid(
        &self,
        r0: usize,
        c0: usize,
        r1: usize,
        c1: usize,
        vis: &mut Visited,
    ) -> Result<Arg, EvalErr> {
        let mut rows = Vec::with_capacity(r1 - r0 + 1);
        for r in r0..=r1 {
            let mut row = Vec::with_capacity(c1 - c0 + 1);
            for c in c0..=c1 {
                row.push(self.resolve_value(r, c, vis)?);
            }
            rows.push(row);
        }
        Ok(Arg::Range(rows))
    }

    // Parse a comma-separated argument list (until RightParen). Scalars become
    // `Arg::Scalar`; `A1:B3`-style ranges keep their shape as `Arg::Range`
    // grids so SUMIF/VLOOKUP/INDEX can see rows and columns (issue #2). Each
    // argument resolves independently — a failing one is captured as `Err`
    // (IFERROR recovers from it; everything else propagates it) and the token
    // cursor resyncs to the next comma / closing paren.
    fn parse_args(
        &self,
        t: &[Token],
        pos: &mut usize,
        vis: &mut Visited,
    ) -> Vec<Result<Arg, EvalErr>> {
        let mut args = Vec::new();
        if matches!(t.get(*pos), Some(Token::RightParen)) {
            *pos += 1;
            return args;
        }
        loop {
            // Sheet-qualified range: `Sheet2!A1:B3` (issue #4). A range
            // inside a larger expression (e.g. `A1:A3*2`) falls through to
            // parse_cmp, which resolves it to an array and broadcasts the
            // operators over it (issue #33).
            let arg: Result<Arg, EvalErr> = if let Some(Token::SheetRange { sheet, from, to }) =
                t.get(*pos).cloned().filter(|_| {
                    matches!(
                        t.get(*pos + 1),
                        Some(Token::Comma) | Some(Token::RightParen) | None
                    )
                }) {
                *pos += 1;
                match self.find_sheet(&sheet) {
                    // Thread the same sheet-keyed visited set through (issue #4).
                    Some(target) => {
                        let (c0, r0) = exp2xy(&from);
                        let (c1, r1) = exp2xy(&to);
                        target.resolve_grid(r0.min(r1), c0.min(c1), r0.max(r1), c0.max(c1), vis)
                    }
                    None => Err(EvalErr::Ref),
                }
            // Range: CellRef ':' CellRef
            } else if let (Some(Token::CellRef(a)), Some(Token::Colon), Some(Token::CellRef(b))) = (
                t.get(*pos).filter(|_| {
                    matches!(
                        t.get(*pos + 3),
                        Some(Token::Comma) | Some(Token::RightParen) | None
                    )
                }),
                t.get(*pos + 1),
                t.get(*pos + 2),
            ) {
                let (c0, r0) = exp2xy(a);
                let (c1, r1) = exp2xy(b);
                *pos += 3;
                self.resolve_grid(r0.min(r1), c0.min(c1), r0.max(r1), c0.max(c1), vis)
            } else if let Some(Token::Name(n)) = t.get(*pos).cloned() {
                // A bare named range as an argument expands to its grid; a name
                // inside a larger expression (e.g. `Rev*2`) falls through to the
                // scalar handling in parse_cmp/parse_factor. A LET / LAMBDA
                // binding wins over a sheet-scoped named range, so the
                // lambda-body case (e.g. `=SUM(c)` where `c` is the lambda
                // parameter) reaches parse_cmp instead of resolving to
                // #NAME? against the workbook.
                let bare = matches!(
                    t.get(*pos + 1),
                    Some(Token::Comma) | Some(Token::RightParen) | None
                );
                let bound = self
                    .let_bindings
                    .borrow()
                    .iter()
                    .rev()
                    .find_map(|f| f.get(&n).cloned());
                if bare && bound.is_none() {
                    *pos += 1;
                    match self.resolve_name(&n) {
                        Some((r0, c0, r1, c1)) => self.resolve_grid(r0, c0, r1, c1, vis),
                        None => Err(EvalErr::Name),
                    }
                } else {
                    self.parse_cmp(t, pos, vis).map(Arg::from_value)
                }
            } else if let Some(Token::Function(f)) = t.get(*pos).cloned() {
                // A bare OFFSET/INDIRECT argument keeps its (possibly multi-cell)
                // Arg::Range so it composes inside SUM(...) etc. If an operator
                // follows the call, it's part of a larger expression and folds
                // to a scalar via parse_cmp (issue #37).
                let upper = f.to_uppercase();
                if matches!(upper.as_str(), "OFFSET" | "INDIRECT") {
                    let save = *pos;
                    *pos += 1; // function name
                    if matches!(t.get(*pos), Some(Token::LeftParen)) {
                        *pos += 1;
                    }
                    let spans = self.arg_spans(t, pos);
                    if matches!(
                        t.get(*pos),
                        Some(Token::Comma) | Some(Token::RightParen) | None
                    ) {
                        self.eval_ref_fn(&upper, t, &spans, vis)
                    } else {
                        *pos = save;
                        self.parse_cmp(t, pos, vis).map(Arg::from_value)
                    }
                } else {
                    self.parse_cmp(t, pos, vis).map(Arg::from_value)
                }
            } else {
                self.parse_cmp(t, pos, vis).map(Arg::from_value)
            };
            args.push(arg);

            // Resync to the next separator: on the happy path the cursor is
            // already there; after a failed parse this skips the remainder of
            // the argument (tracking nesting depth).
            let mut depth = 0usize;
            loop {
                match t.get(*pos) {
                    Some(Token::Comma) if depth == 0 => {
                        *pos += 1;
                        break; // next argument
                    }
                    Some(Token::RightParen) if depth == 0 => {
                        *pos += 1;
                        return args;
                    }
                    Some(Token::LeftParen) => {
                        depth += 1;
                        *pos += 1;
                    }
                    Some(Token::RightParen) => {
                        depth -= 1;
                        *pos += 1;
                    }
                    Some(_) => {
                        *pos += 1;
                    }
                    None => return args,
                }
            }
        }
    }

    /// Split the argument list (cursor just past `(`) into per-argument token
    /// spans `[start, end)` WITHOUT evaluating, advancing `pos` past the
    /// matching `)`. Used by lazy functions (IF/IFS/CHOOSE) so a not-taken
    /// branch is never evaluated (issue #38). Nested parens are tracked so a
    /// comma inside a sub-call doesn't split an argument.
    fn arg_spans(&self, t: &[Token], pos: &mut usize) -> Vec<(usize, usize)> {
        let mut spans = Vec::new();
        if matches!(t.get(*pos), Some(Token::RightParen)) {
            *pos += 1;
            return spans;
        }
        let mut start = *pos;
        let mut depth = 0usize;
        loop {
            match t.get(*pos) {
                Some(Token::LeftParen) | Some(Token::LeftBrace) => {
                    depth += 1;
                    *pos += 1;
                }
                Some(Token::RightParen) | Some(Token::RightBrace) if depth > 0 => {
                    depth -= 1;
                    *pos += 1;
                }
                Some(Token::RightParen) => {
                    spans.push((start, *pos));
                    *pos += 1;
                    return spans;
                }
                Some(Token::Comma) if depth == 0 => {
                    spans.push((start, *pos));
                    *pos += 1;
                    start = *pos;
                }
                Some(_) => *pos += 1,
                None => {
                    spans.push((start, *pos));
                    return spans;
                }
            }
        }
    }

    /// Evaluate the sub-expression in `t[span.0..span.1]` to a scalar value.
    fn eval_span(
        &self,
        t: &[Token],
        span: (usize, usize),
        vis: &mut Visited,
    ) -> Result<Value, EvalErr> {
        let mut p = 0usize;
        self.parse_cmp(&t[span.0..span.1], &mut p, vis)
    }

    /// Short-circuit IF/IFS/CHOOSE: evaluate the condition/index first, then
    /// only the chosen branch (issue #38). Semantics match the former eager
    /// arms in `apply_special_function`.
    fn eval_lazy(
        &self,
        name: &str,
        t: &[Token],
        spans: &[(usize, usize)],
        vis: &mut Visited,
    ) -> Result<Value, EvalErr> {
        match name {
            "IF" => {
                let Some(cond) = spans.first() else {
                    return Err(EvalErr::Value);
                };
                let branch = if self.eval_span(t, *cond, vis)?.is_truthy() {
                    spans.get(1)
                } else {
                    spans.get(2)
                };
                match branch {
                    // A missing branch yields FALSE (0), matching Excel/the
                    // former eager IF.
                    Some(&s) => self.eval_span(t, s, vis),
                    None => Ok(Value::Number(0.0)),
                }
            }
            "IFS" => {
                let mut i = 0;
                while i + 1 < spans.len() {
                    if self.eval_span(t, spans[i], vis)?.is_truthy() {
                        return self.eval_span(t, spans[i + 1], vis);
                    }
                    i += 2;
                }
                Err(EvalErr::Na) // no condition matched
            }
            "CHOOSE" => {
                let Some(idx_span) = spans.first() else {
                    return Err(EvalErr::Value);
                };
                let idx = self.eval_span(t, *idx_span, vis)?.as_number() as usize;
                if idx < 1 || idx >= spans.len() {
                    return Err(EvalErr::Value);
                }
                self.eval_span(t, spans[idx], vis)
            }
            _ => Err(EvalErr::Value),
        }
    }

    /// Extract a reference's bounds `(sri, sci, eri, eci)` from a single
    /// argument's token span — a `CellRef`, a `CellRef:CellRef` range, or a
    /// named range. ROW/COLUMN/OFFSET need the argument's coordinates, not its
    /// value, so they read the span directly rather than via `parse_args`
    /// (issue #37).
    fn span_ref(&self, t: &[Token], span: (usize, usize)) -> Option<(usize, usize, usize, usize)> {
        match &t[span.0..span.1] {
            [Token::CellRef(a)] => {
                let (c, r) = exp2xy(a);
                Some((r, c, r, c))
            }
            [Token::CellRef(a), Token::Colon, Token::CellRef(b)] => {
                let (c0, r0) = exp2xy(a);
                let (c1, r1) = exp2xy(b);
                Some((r0.min(r1), c0.min(c1), r0.max(r1), c0.max(c1)))
            }
            [Token::Name(n)] => self.resolve_name(n),
            _ => None,
        }
    }

    /// LET(name, value, body) (Phase 3.2). Builds a name→Value map
    /// from the (name, value) pairs, pushes it onto the LET-binding
    /// stack, evaluates the body span with the top frame visible,
    /// then pops the frame. Spans come from `arg_spans` so the
    /// body sees the bindings without round-tripping through
    /// `Value` (LET values can be anything, including arrays).
    ///
    /// Argument shape: an even number of name/value pairs followed
    /// by one body expression. Returns `#VALUE!` on:
    /// * any name slot that's not a single `Token::Name` (we
    ///   disallow string-quoted names to keep the binding set
    ///   alphanumeric — `"foo"` is a string, `foo` is a name),
    /// * any name slot that's empty,
    /// * odd number of slots before the body.
    fn eval_let(
        &self,
        t: &[Token],
        spans: &[(usize, usize)],
        vis: &mut Visited,
    ) -> Result<Value, EvalErr> {
        if spans.len() < 3 {
            return Err(EvalErr::Value);
        }
        let body_span = spans[spans.len() - 1];
        let pairs = &spans[..spans.len() - 1];
        if !pairs.len().is_multiple_of(2) {
            return Err(EvalErr::Value);
        }
        self.let_bindings.borrow_mut().push(HashMap::new());
        for pair in pairs.chunks_exact(2) {
            let name_span = pair[0];
            let value_span = pair[1];
            // The name slot must be exactly one Token::Name; empty
            // spans (which would mean "name, ,value, body") are
            // rejected.
            if name_span.0 == name_span.1 {
                return Err(EvalErr::Value);
            }
            let name_tok = t.get(name_span.0).ok_or(EvalErr::Value)?;
            let Token::Name(name) = name_tok else {
                return Err(EvalErr::Value);
            };
            let value = self.eval_span(t, value_span, vis)?;
            // Insert into the TOP frame so later iterations and
            // the body span see this binding.
            if let Some(top) = self.let_bindings.borrow_mut().last_mut() {
                top.insert(name.clone(), value);
            }
        }
        // Push an empty frame first so subsequent value spans
        // see earlier bindings. Pop regardless of result so a
        // body error doesn't leak the bindings to the outer scope.
        let result = self.eval_span(t, body_span, vis);
        self.let_bindings.borrow_mut().pop();
        result
    }

    /// LAMBDA(p1, p2, …, body) (Phase 3.3). Capture the parameter
    /// names + the body token stream and return a `Value::Lambda`.
    /// Body tokens are sliced out of `t` and stored by value, so
    /// the lambda can outlive the surrounding parser span.
    ///
    /// Arg shape: 1+ parameter names followed by one body span.
    /// Empty parameter list is allowed (`=LAMBDA(body)`) for
    /// constant expressions — calling it just evaluates `body`.
    fn eval_lambda(&self, t: &[Token], spans: Vec<(usize, usize)>) -> Result<Value, EvalErr> {
        if spans.len() < 2 {
            return Err(EvalErr::Value);
        }
        let body_span = spans[spans.len() - 1];
        let mut params: Vec<String> = Vec::with_capacity(spans.len() - 1);
        for span in &spans[..spans.len() - 1] {
            if span.0 == span.1 {
                return Err(EvalErr::Value);
            }
            let name_tok = t.get(span.0).ok_or(EvalErr::Value)?;
            let Token::Name(name) = name_tok else {
                return Err(EvalErr::Value);
            };
            params.push(name.clone());
        }
        let body_tokens = t[body_span.0..body_span.1].to_vec();
        Ok(Value::Lambda {
            params,
            body_tokens,
        })
    }

    /// Invoke a `Value::Lambda` with a list of argument values. Pushes
    /// a temporary LET frame so the lambda's parameter names resolve
    /// to the supplied arguments, evaluates the body, then pops the
    /// frame. Used by MAP (Phase 3.3); REDUCE / BYROW / BYCOL will
    /// reuse this in their follow-ups.
    fn call_lambda(
        &self,
        lambda: &Value,
        args: Vec<Value>,
        vis: &mut Visited,
    ) -> Result<Value, EvalErr> {
        let (params, body_tokens) = match lambda {
            Value::Lambda {
                params,
                body_tokens,
            } => (params, body_tokens),
            _ => return Err(EvalErr::Value),
        };
        if args.len() != params.len() {
            return Err(EvalErr::Value);
        }
        let mut frame: HashMap<String, Value> = HashMap::new();
        for (p, v) in params.iter().zip(args) {
            frame.insert(p.clone(), v);
        }
        eprintln!(
            "DBG call_lambda: params={:?}, body_tokens={:?}",
            params, body_tokens
        );
        self.let_bindings.borrow_mut().push(frame);
        let mut p = 0usize;
        let result = self.parse_cmp(body_tokens, &mut p, vis);
        eprintln!("DBG call_lambda result={:?}", result);
        self.let_bindings.borrow_mut().pop();
        result
    }

    /// MAP(array, lambda) (Phase 3.3). Apply the lambda to every
    /// element of `array`. The lambda must take exactly one
    /// argument; the result is a same-shape array with each cell
    /// replaced by `lambda(element)`.
    fn eval_map(
        &self,
        t: &[Token],
        spans: &[(usize, usize)],
        vis: &mut Visited,
    ) -> Result<Value, EvalErr> {
        if spans.len() != 2 {
            return Err(EvalErr::Value);
        }
        let array = self.eval_span(t, spans[0], vis)?;
        let lambda = self.eval_span(t, spans[1], vis)?;
        let Value::Lambda { params, .. } = &lambda else {
            return Err(EvalErr::Value);
        };
        if params.len() != 1 {
            return Err(EvalErr::Value);
        }
        match array {
            Value::Array(rows) => {
                let mut out: Vec<Vec<Value>> = Vec::with_capacity(rows.len());
                for row in rows {
                    let mut new_row: Vec<Value> = Vec::with_capacity(row.len());
                    for cell in row {
                        let v = self.call_lambda(&lambda, vec![cell], vis)?;
                        new_row.push(v);
                    }
                    out.push(new_row);
                }
                Ok(Value::Array(out))
            }
            Value::Blank => Ok(Value::Array(Vec::new())),
            // Scalar argument: MAP applies the lambda once and
            // returns a 1×1 array. Matches Excel's behaviour for
            // =MAP(5, LAMBDA(x, x*2)) → {10}.
            other => {
                let v = self.call_lambda(&lambda, vec![other], vis)?;
                Ok(Value::Array(vec![vec![v]]))
            }
        }
    }

    /// REDUCE(initial, array, LAMBDA(acc, x, body)) (Phase 3.4).
    /// Fold-style accumulation: starts with `initial`, calls the
    /// lambda once per cell of `array`, threading the prior
    /// return value as `acc`. Returns the final accumulator. The
    /// lambda must take exactly 2 parameters (acc, x); otherwise
    /// #VALUE!.
    fn eval_reduce(
        &self,
        t: &[Token],
        spans: &[(usize, usize)],
        vis: &mut Visited,
    ) -> Result<Value, EvalErr> {
        if spans.len() != 3 {
            return Err(EvalErr::Value);
        }
        let initial = self.eval_span(t, spans[0], vis)?;
        let array = self.eval_span(t, spans[1], vis)?;
        let lambda = self.eval_span(t, spans[2], vis)?;
        let Value::Lambda { params, .. } = &lambda else {
            return Err(EvalErr::Value);
        };
        if params.len() != 2 {
            return Err(EvalErr::Value);
        }
        let Value::Array(rows) = array else {
            return Err(EvalErr::Value);
        };
        let mut acc = initial;
        for row in rows.iter().flatten() {
            acc = self.call_lambda(&lambda, vec![acc, row.clone()], vis)?;
        }
        Ok(acc)
    }

    /// BYROW(array, LAMBDA(row, body)) (Phase 3.4). Each row of
    /// `array` is passed to the lambda as a one-dimensional array
    /// argument; the results form a column vector. A 1-D array
    /// has one row, so BYROW returns a 1×1 array of one result.
    fn eval_byrow(
        &self,
        t: &[Token],
        spans: &[(usize, usize)],
        vis: &mut Visited,
    ) -> Result<Value, EvalErr> {
        self.eval_by_dimension(t, spans, vis, /*by_column=*/ false)
    }

    /// BYCOL(array, LAMBDA(col, body)) (Phase 3.4). Like BYROW
    /// but iterates the inner dimension. A 1×N range gets N
    /// column calls; an M×N range walks one column at a time,
    /// yielding M results.
    fn eval_bycol(
        &self,
        t: &[Token],
        spans: &[(usize, usize)],
        vis: &mut Visited,
    ) -> Result<Value, EvalErr> {
        self.eval_by_dimension(t, spans, vis, /*by_column=*/ true)
    }

    /// Shared body for BYROW / BYCOL. Iterates the requested
    /// dimension; the lambda must take exactly one argument.
    fn eval_by_dimension(
        &self,
        t: &[Token],
        spans: &[(usize, usize)],
        vis: &mut Visited,
        by_column: bool,
    ) -> Result<Value, EvalErr> {
        if spans.len() != 2 {
            return Err(EvalErr::Value);
        }
        let array = self.eval_span(t, spans[0], vis)?;
        let lambda = self.eval_span(t, spans[1], vis)?;
        let Value::Lambda { params, .. } = &lambda else {
            return Err(EvalErr::Value);
        };
        if params.len() != 1 {
            return Err(EvalErr::Value);
        }
        let Value::Array(rows) = array else {
            return Err(EvalErr::Value);
        };
        eprintln!(
            "DBG by_dim: rows.len={}, rows[0].len={}, params={:?}",
            rows.len(),
            rows.first().map(|r| r.len()).unwrap_or(0),
            params
        );
        if by_column {
            if rows.is_empty() {
                return Ok(Value::Array(Vec::new()));
            }
            let ncols = rows[0].len();
            let mut out: Vec<Vec<Value>> = Vec::with_capacity(ncols);
            for col in 0..ncols {
                let column: Vec<Value> = rows.iter().map(|r| r[col].clone()).collect();
                let v = self.call_lambda(&lambda, vec![Value::Array(vec![column])], vis)?;
                out.push(vec![v]);
            }
            // BYCOL yields one value per column, laid out as a
            // single column: ncols rows × 1 column.
            let one_col: Vec<Vec<Value>> = out.into_iter().map(|v| vec![v[0].clone()]).collect();
            Ok(Value::Array(one_col))
        } else {
            let mut out: Vec<Vec<Value>> = Vec::with_capacity(rows.len());
            for row in rows {
                let v = self.call_lambda(&lambda, vec![Value::Array(vec![row])], vis)?;
                out.push(vec![v]);
            }
            Ok(Value::Array(out))
        }
    }

    /// MAKEARRAY(rows, cols, LAMBDA(i, j, body)) (Phase 3.4).
    /// Builds an `rows × cols` array where each cell is the
    /// lambda evaluated with `(i, j)` (1-based indices). Lambda
    /// must take exactly 2 parameters.
    fn eval_makearray(
        &self,
        t: &[Token],
        spans: &[(usize, usize)],
        vis: &mut Visited,
    ) -> Result<Value, EvalErr> {
        if spans.len() != 3 {
            return Err(EvalErr::Value);
        }
        let nrows = self.eval_span(t, spans[0], vis)?.as_number() as usize;
        let ncols = self.eval_span(t, spans[1], vis)?.as_number() as usize;
        let lambda = self.eval_span(t, spans[2], vis)?;
        let Value::Lambda { params, .. } = &lambda else {
            return Err(EvalErr::Value);
        };
        if params.len() != 2 {
            return Err(EvalErr::Value);
        }
        let mut grid: Vec<Vec<Value>> = Vec::with_capacity(nrows);
        for i in 1..=nrows {
            let mut row: Vec<Value> = Vec::with_capacity(ncols);
            for j in 1..=ncols {
                let v = self.call_lambda(
                    &lambda,
                    vec![Value::Number(i as f64), Value::Number(j as f64)],
                    vis,
                )?;
                row.push(v);
            }
            grid.push(row);
        }
        Ok(Value::Array(grid))
    }

    /// Position / reference functions: ROW, COLUMN, ADDRESS, OFFSET, INDIRECT
    /// (issue #37). Returns an `Arg` so OFFSET/INDIRECT can yield a multi-cell
    /// `Arg::Range` that composes inside `SUM(...)`; ROW/COLUMN/ADDRESS yield a
    /// scalar. `arg_spans` (not `parse_args`) supplies the spans so the ref
    /// arguments keep their coordinates.
    fn eval_ref_fn(
        &self,
        name: &str,
        t: &[Token],
        spans: &[(usize, usize)],
        vis: &mut Visited,
    ) -> Result<Arg, EvalErr> {
        match name {
            "ROW" | "COLUMN" => {
                let (r, c) = if spans.is_empty() {
                    self.eval_cell.get() // caller's position when the arg is omitted
                } else {
                    let (r0, c0, _, _) = self.span_ref(t, spans[0]).ok_or(EvalErr::Ref)?;
                    (r0, c0)
                };
                let n = if name == "ROW" { r + 1 } else { c + 1 }; // A1 is row 1 / col 1
                Ok(Arg::Scalar(Value::Number(n as f64)))
            }
            "ADDRESS" => {
                if spans.len() < 2 {
                    return Err(EvalErr::Value);
                }
                let row = self.eval_span(t, spans[0], vis)?.as_number() as i64;
                let col = self.eval_span(t, spans[1], vis)?.as_number() as i64;
                if row < 1 || col < 1 {
                    return Err(EvalErr::Value);
                }
                let abs = match spans.get(2) {
                    Some(s) => self.eval_span(t, *s, vis)?.as_number() as i64,
                    None => 1,
                };
                Ok(Arg::Scalar(Value::Text(format_address(
                    row as usize,
                    col as usize,
                    abs,
                ))))
            }
            "OFFSET" => {
                if spans.len() < 3 {
                    return Err(EvalErr::Value);
                }
                let (r0, c0, r1, c1) = self.span_ref(t, spans[0]).ok_or(EvalErr::Ref)?;
                let drows = self.eval_span(t, spans[1], vis)?.as_number() as i64;
                let dcols = self.eval_span(t, spans[2], vis)?.as_number() as i64;
                let height = match spans.get(3) {
                    Some(s) => self.eval_span(t, *s, vis)?.as_number() as i64,
                    None => (r1 - r0 + 1) as i64,
                };
                let width = match spans.get(4) {
                    Some(s) => self.eval_span(t, *s, vis)?.as_number() as i64,
                    None => (c1 - c0 + 1) as i64,
                };
                let nr0 = r0 as i64 + drows;
                let nc0 = c0 as i64 + dcols;
                if nr0 < 0 || nc0 < 0 || height < 1 || width < 1 {
                    return Err(EvalErr::Ref);
                }
                self.resolve_grid(
                    nr0 as usize,
                    nc0 as usize,
                    (nr0 + height - 1) as usize,
                    (nc0 + width - 1) as usize,
                    vis,
                )
            }
            "INDIRECT" => {
                if spans.is_empty() {
                    return Err(EvalErr::Value);
                }
                let s = self.eval_span(t, spans[0], vis)?.as_text();
                let (r0, c0, r1, c1) = parse_a1_ref(&s).ok_or(EvalErr::Ref)?;
                self.resolve_grid(r0, c0, r1, c1, vis)
            }
            // SUBTOTAL(function_num, ref1, [ref2, …]) — issue #30. The
            // 101–111 variants skip hidden rows (collapsed outline groups,
            // filtered-out or manually hidden rows all share the hide flag,
            // so all are skipped); 1–11 include every row.
            "SUBTOTAL" => {
                if spans.len() < 2 {
                    return Err(EvalErr::Value);
                }
                let f = self.eval_span(t, spans[0], vis)?.as_number() as i64;
                let (skip_hidden, func) = if (101..=111).contains(&f) {
                    (true, f - 100)
                } else if (1..=11).contains(&f) {
                    (false, f)
                } else {
                    return Err(EvalErr::Value);
                };
                let mut nums: Vec<f64> = Vec::new();
                let mut nonblank = 0usize;
                for span in &spans[1..] {
                    let (r0, c0, r1, c1) = self.span_ref(t, *span).ok_or(EvalErr::Ref)?;
                    for r in r0..=r1 {
                        if skip_hidden && self.is_row_hidden(r) {
                            continue;
                        }
                        for c in c0..=c1 {
                            match self.resolve_value(r, c, vis)? {
                                Value::Number(n) => {
                                    nums.push(n);
                                    nonblank += 1;
                                }
                                Value::Text(s) if !s.trim().is_empty() => nonblank += 1,
                                _ => {}
                            }
                        }
                    }
                }
                let n = nums.len() as f64;
                let mean = || nums.iter().sum::<f64>() / n;
                // Sample (n-1) / population (n) variance; #DIV/0! when the
                // divisor would be zero, matching Excel.
                let var = |pop: bool| -> Result<f64, EvalErr> {
                    let d = if pop { n } else { n - 1.0 };
                    if d <= 0.0 {
                        return Err(EvalErr::Div0);
                    }
                    let m = mean();
                    Ok(nums.iter().map(|x| (x - m) * (x - m)).sum::<f64>() / d)
                };
                let v = match func {
                    1 => {
                        if nums.is_empty() {
                            return Err(EvalErr::Div0);
                        }
                        mean()
                    }
                    2 => n,
                    3 => nonblank as f64,
                    4 => {
                        if nums.is_empty() {
                            0.0
                        } else {
                            nums.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
                        }
                    }
                    5 => {
                        if nums.is_empty() {
                            0.0
                        } else {
                            nums.iter().cloned().fold(f64::INFINITY, f64::min)
                        }
                    }
                    6 => {
                        if nums.is_empty() {
                            0.0
                        } else {
                            nums.iter().product()
                        }
                    }
                    7 => var(false)?.sqrt(),
                    8 => var(true)?.sqrt(),
                    9 => nums.iter().sum(),
                    10 => var(false)?,
                    11 => var(true)?,
                    _ => return Err(EvalErr::Value),
                };
                Ok(Arg::Scalar(Value::Number(v)))
            }
            _ => Err(EvalErr::Value),
        }
    }

    /// CELL(info_type, [reference]) — returns information about a cell or the
    /// sheet (issue #14). Supports "address", "col", "row", "filename" (sheet
    /// name), and "contents".
    fn eval_cell_fn(&self, args: &[Result<Arg, EvalErr>]) -> Result<Value, EvalErr> {
        let info_type = match args.first() {
            Some(Ok(a)) => a.to_scalar().as_text().to_lowercase(),
            Some(Err(e)) => return Err(*e),
            None => return Err(EvalErr::Value),
        };
        // Determine the target cell: use the reference arg, or the calling cell.
        let (ri, ci) = if args.len() >= 2 {
            match self.span_ref_from_arg(&args[1]) {
                Some((r, c, _, _)) => (r, c),
                None => {
                    return Err(EvalErr::Ref);
                }
            }
        } else {
            self.eval_cell.get()
        };
        match info_type.as_str() {
            "address" => Ok(Value::Text(crate::renderer::alphabets::xy2expr(ci, ri))),
            "col" => Ok(Value::Number((ci + 1) as f64)),
            "row" => Ok(Value::Number((ri + 1) as f64)),
            "filename" => Ok(Value::Text(self.name.clone())),
            "contents" => {
                let v = self.cell_display_value(ri, ci);
                Ok(Value::Text(v))
            }
            "type" => {
                let t = self.get_cell_text(ri, ci);
                if t.is_empty() {
                    Ok(Value::Text("b".into())) // blank
                } else if t.starts_with('=') {
                    Ok(Value::Text("l".into())) // label (formula)
                } else {
                    Ok(Value::Text("v".into())) // value
                }
            }
            "format" => Ok(Value::Text(String::new())), // not implemented
            "width" => Ok(Value::Number(
                (self.get_col_width(ci) / self.zoom()).round(),
            )),
            _ => Err(EvalErr::Value),
        }
    }

    /// Like `span_ref` but for an `Arg` result instead of raw tokens.
    fn span_ref_from_arg(
        &self,
        arg: &Result<Arg, EvalErr>,
    ) -> Option<(usize, usize, usize, usize)> {
        match arg {
            Ok(Arg::Scalar(Value::Text(s))) => {
                if let Ok(cr) = crate::core::cell_range::CellRange::from_str(s) {
                    Some((cr.sri, cr.sci, cr.eri, cr.eci))
                } else {
                    None
                }
            }
            _ => None,
        }
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
        self.mark_spills_dirty();
        self.rows.entry(ri).or_default().set_cell(ci, cell);
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
        self.mark_spills_dirty();
        self.named_ranges
            .insert(name.to_uppercase(), range_expr.to_string());
    }

    /// The range expression for a name (e.g. `"B2:B3"`), if defined.
    pub fn get_named_range(&self, name: &str) -> Option<String> {
        self.named_ranges.get(&name.to_uppercase()).cloned()
    }

    /// Remove a named range; returns whether it existed.
    pub fn remove_named_range(&mut self, name: &str) -> bool {
        self.mark_spills_dirty();
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

    // --- Excel-style tables & structured references (issue #34) ---

    /// The table covering `(ri, ci)`, if any.
    pub fn table_at(&self, ri: usize, ci: usize) -> Option<&Table> {
        self.tables.iter().find(|t| t.contains(ri, ci))
    }

    /// Find a table by name (case-insensitive), searching this sheet first
    /// and then the whole workbook — table names are workbook-global, like
    /// Excel's. An empty name is the in-table shorthand (`[@Col]`): it means
    /// "the table containing the cell whose formula is being evaluated".
    fn find_table(&self, name: &str) -> Option<(DataProxy, Table)> {
        if name.is_empty() {
            let (ri, ci) = self.eval_cell.get();
            let t = self.tables.iter().find(|t| t.contains(ri, ci))?.clone();
            return Some((self.clone(), t));
        }
        let upper = name.to_uppercase();
        if let Some(t) = self.tables.iter().find(|t| t.name.to_uppercase() == upper) {
            return Some((self.clone(), t.clone()));
        }
        let reg = self.sheets.as_ref()?.upgrade()?;
        let found = reg.borrow().iter().find_map(|d| {
            d.tables
                .iter()
                .find(|t| t.name.to_uppercase() == upper)
                .map(|t| (d.clone(), t.clone()))
        });
        found
    }

    /// The column index for a header titled `name` (trimmed,
    /// case-insensitive) in `t`, read from the owner sheet's header row.
    fn table_col_by_name(owner: &DataProxy, t: &Table, name: &str) -> Option<usize> {
        let want = name.trim().to_uppercase();
        (t.sci..=t.eci)
            .find(|&c| owner.get_cell_text(t.header_row(), c).trim().to_uppercase() == want)
    }

    /// Resolve a structured reference (issue #34) to an argument: a grid for
    /// range-shaped specs, a scalar for the this-row form `[@Col]`.
    ///
    /// Supported spec shapes (the raw bracket interior from the tokenizer):
    /// `Col`, `` (empty — data body), `#All`, `#Headers`, `#Totals`,
    /// `#Data`, `@` (this row), `@Col` (this row ∩ column), and the
    /// bracketed combination `[#Item],[Col]`. Unknown tables, columns, or a
    /// missing totals row are #REF!; `@` outside the table's data rows is
    /// #VALUE!.
    fn resolve_struct_ref(
        &self,
        table: &str,
        spec: &str,
        vis: &mut Visited,
    ) -> Result<Arg, EvalErr> {
        let (owner, t) = self.find_table(table).ok_or(EvalErr::Ref)?;

        // Split `[#Totals],[Amount]`-style specs into segments at top-level
        // commas, dropping the per-segment brackets; a plain spec is one
        // segment.
        let mut item: Option<String> = None;
        let mut column: Option<String> = None;
        let mut this_row = false;
        let mut depth = 0usize;
        let mut seg = String::new();
        let mut segs: Vec<String> = Vec::new();
        for ch in spec.chars() {
            match ch {
                '[' => depth += 1,
                ']' => depth = depth.saturating_sub(1),
                ',' if depth == 0 => {
                    segs.push(seg.trim().to_string());
                    seg.clear();
                }
                _ => seg.push(ch),
            }
        }
        segs.push(seg.trim().to_string());
        for s in segs {
            if s.is_empty() {
                continue;
            } else if let Some(it) = s.strip_prefix('#') {
                item = Some(it.trim().to_uppercase());
            } else if let Some(col) = s.strip_prefix('@') {
                this_row = true;
                if !col.trim().is_empty() {
                    column = Some(col.trim().to_string());
                }
            } else {
                column = Some(s);
            }
        }

        // Column bounds.
        let (c0, c1) = match &column {
            Some(name) => {
                let c = Self::table_col_by_name(&owner, &t, name).ok_or(EvalErr::Ref)?;
                (c, c)
            }
            None => (t.sci, t.eci),
        };

        // Row bounds.
        if this_row {
            // `[@…]` only means something for a formula inside the table's
            // own sheet, on one of its data rows.
            let (ri, _) = self.eval_cell.get();
            let (first, last) = t.data_rows().ok_or(EvalErr::Ref)?;
            if owner.name != self.name || ri < first || ri > last {
                return Err(EvalErr::Value);
            }
            if c0 == c1 {
                return Ok(Arg::Scalar(owner.resolve_value(ri, c0, vis)?));
            }
            return owner.resolve_grid(ri, c0, ri, c1, vis);
        }
        let (r0, r1) = match item.as_deref() {
            Some("ALL") => (t.sri, t.eri),
            Some("HEADERS") => (t.header_row(), t.header_row()),
            Some("TOTALS") => {
                let r = t.totals_row_index().ok_or(EvalErr::Ref)?;
                (r, r)
            }
            Some("DATA") | None => t.data_rows().ok_or(EvalErr::Ref)?,
            Some(_) => return Err(EvalErr::Ref),
        };
        owner.resolve_grid(r0, c0, r1, c1, vis)
    }

    /// The next free `TableN` name, unique workbook-wide (case-insensitive).
    fn next_table_name(&self) -> String {
        let mut taken: HashSet<String> =
            self.tables.iter().map(|t| t.name.to_uppercase()).collect();
        if let Some(reg) = self.sheets.as_ref().and_then(Weak::upgrade) {
            for d in reg.borrow().iter() {
                taken.extend(d.tables.iter().map(|t| t.name.to_uppercase()));
            }
        }
        let mut n = 1usize;
        loop {
            if !taken.contains(&format!("TABLE{n}")) {
                return format!("Table{n}");
            }
            n += 1;
        }
    }

    /// "Format as Table" (issue #34): the range's first row becomes the
    /// header (empty header cells are filled with Column1, Column2, …), the
    /// table is registered under a fresh `TableN` name, and the sheet
    /// autofilter is pointed at it so the headers get dropdowns (issue #10).
    /// A single-row range grows one data row. Overlapping an existing table
    /// is a no-op that returns the existing name, like Excel refusing to
    /// nest tables.
    pub fn format_as_table(&mut self, range: &CellRange) -> String {
        let (sri, sci, mut eri, eci) = (range.sri, range.sci, range.eri, range.eci);
        if eri == sri {
            eri = sri + 1;
        }
        if let Some(t) = self
            .tables
            .iter()
            .find(|t| t.sri <= eri && sri <= t.eri && t.sci <= eci && sci <= t.eci)
        {
            return t.name.clone();
        }
        let name = self.next_table_name();
        for (k, c) in (sci..=eci).enumerate() {
            if self.get_cell_text(sri, c).trim().is_empty() {
                self.set_cell_text(sri, c, &format!("Column{}", k + 1));
            }
        }
        self.tables.push(Table::new(&name, sri, sci, eri, eci));
        self.auto_filter.ref_ = Some(CellRange::new(sri, sci, eri, eci).to_string());
        self.mark_spills_dirty();
        name
    }

    /// Toggle the table's totals row (issue #34). Enabling extends the table
    /// one row down, labels it "Total" under the first column, and writes
    /// `=SUBTOTAL(9, …)` under the last column — SUBTOTAL so rows hidden by
    /// the header filter drop out of the sum. Disabling clears those cells
    /// and shrinks the range back.
    pub fn toggle_table_totals(&mut self, name: &str) {
        let upper = name.to_uppercase();
        let Some(idx) = self
            .tables
            .iter()
            .position(|t| t.name.to_uppercase() == upper)
        else {
            return;
        };
        let t = self.tables[idx].clone();
        if t.totals_row {
            let r = t.eri;
            for c in t.sci..=t.eci {
                self.delete_cell(r, c);
            }
            let nt = &mut self.tables[idx];
            nt.totals_row = false;
            nt.eri -= 1;
        } else {
            let r = t.eri + 1;
            self.set_cell_text(r, t.sci, "Total");
            if t.eci > t.sci {
                let from = crate::renderer::alphabets::xy2expr(t.eci, t.sri + 1);
                let to = crate::renderer::alphabets::xy2expr(t.eci, t.eri);
                self.set_cell_text(r, t.eci, &format!("=SUBTOTAL(9,{from}:{to})"));
            }
            let nt = &mut self.tables[idx];
            nt.totals_row = true;
            nt.eri += 1;
        }
        self.mark_spills_dirty();
    }

    /// "Convert to Range" (issue #34): drop the table layer, keeping every
    /// cell. The header autofilter is cleared when it was the table's.
    pub fn convert_table_to_range(&mut self, name: &str) {
        let upper = name.to_uppercase();
        let Some(idx) = self
            .tables
            .iter()
            .position(|t| t.name.to_uppercase() == upper)
        else {
            return;
        };
        let t = self.tables.remove(idx);
        if let Some(r) = self.auto_filter.range() {
            if r.sri == t.sri && r.sci == t.sci {
                self.auto_filter.ref_ = None;
                self.auto_filter.filters.clear();
                self.auto_filter.sort.clear();
                // Filtered-out rows would otherwise stay hidden forever.
                for ri in t.sri..=t.eri {
                    self.set_row_hidden(ri, false);
                }
            }
        }
        self.mark_spills_dirty();
    }

    /// Auto-expansion (issue #34): after committing a non-empty value in the
    /// row just below a table's data body (within its columns) or the column
    /// just right of it (within its rows), the table grows to absorb the
    /// cell, Excel-style. New columns get a generated header. Tables with a
    /// totals row don't grow downward (the cell below the body is the totals
    /// row itself).
    pub fn maybe_expand_tables(&mut self, ri: usize, ci: usize) {
        if self.get_cell_text(ri, ci).trim().is_empty() {
            return;
        }
        let mut grown: Option<usize> = None;
        for idx in 0..self.tables.len() {
            let t = self.tables[idx].clone();
            if !t.totals_row && ri == t.eri + 1 && ci >= t.sci && ci <= t.eci {
                self.tables[idx].eri += 1;
                grown = Some(idx);
            } else if ci == t.eci + 1 && ri >= t.sri && ri <= t.growth_row() {
                self.tables[idx].eci += 1;
                if ri != t.sri && self.get_cell_text(t.sri, ci).trim().is_empty() {
                    let coln = ci - t.sci + 1;
                    self.set_cell_text(t.sri, ci, &format!("Column{coln}"));
                }
                grown = Some(idx);
            }
        }
        if let Some(idx) = grown {
            let t = self.tables[idx].clone();
            // Keep the header dropdowns tracking the table when the
            // autofilter is anchored at it.
            if let Some(r) = self.auto_filter.range() {
                if r.sri == t.sri && r.sci == t.sci {
                    self.auto_filter.ref_ =
                        Some(CellRange::new(t.sri, t.sci, t.growth_row(), t.eci).to_string());
                }
            }
            self.mark_spills_dirty();
        }
    }

    /// Overlay table visuals onto a cell's style (issue #34) — called by the
    /// render path before conditional formats, so CF rules win. The fixed
    /// look approximates Excel's default "TableStyleMedium2": blue header
    /// with bold white text, banded data rows, bold shaded totals row.
    pub fn apply_table_style(&self, ri: usize, ci: usize, style: &mut Style) {
        let Some(t) = self.table_at(ri, ci) else {
            return;
        };
        if ri == t.header_row() {
            style.bgcolor = Some("#4472c4".to_string());
            style.color = "#ffffff".to_string();
            style.bold = true;
        } else if t.totals_row_index() == Some(ri) {
            style.bgcolor = Some("#d9e1f2".to_string());
            style.bold = true;
        } else if t.banded {
            if let Some((first, _)) = t.data_rows() {
                // Shade odd body rows, but never over an explicit cell fill.
                let plain = matches!(
                    style.bgcolor.as_deref(),
                    None | Some("#ffffff") | Some("#fff")
                );
                if (ri - first) % 2 == 1 && plain {
                    style.bgcolor = Some("#d9e1f2".to_string());
                }
            }
        }
    }

    pub fn delete_cell(&mut self, ri: usize, ci: usize) {
        self.mark_spills_dirty();
        if let Some(row) = self.rows.get_mut(&ri) {
            row.delete_cell(ci);
        }
    }

    /// Insert `n` blank rows at `at`, shifting existing rows down.
    pub fn insert_row(&mut self, at: usize, n: usize) {
        self.mark_spills_dirty();
        let mut new_rows = HashMap::new();
        for (ri, row) in self.rows.drain() {
            let nk = if ri >= at { ri + n } else { ri };
            new_rows.insert(nk, row);
        }
        self.rows = new_rows;
        self.row_count += n;
        self.merges.shift("row", at, n as isize, |_, _, _, _| {});
        self.adjust_all_formulas(true, at, n as isize, None);
        shift_groups_for_insert(&mut self.row_groups, at, n);
        crate::core::table::shift_tables_for_insert(&mut self.tables, true, at, n);
    }

    /// Delete the row at `at`, shifting later rows up.
    pub fn delete_row(&mut self, at: usize) {
        self.mark_spills_dirty();
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
        shift_groups_for_delete(&mut self.row_groups, at);
        crate::core::table::shift_tables_for_delete(&mut self.tables, true, at);
    }

    /// Insert SUBTOTAL rows into `r0..=r1` (issue #30): at each change of
    /// value in the key column `c0`, a "<key> Total" row with
    /// `=SUBTOTAL(9, …)` per numeric column `c0+1..=c1` is inserted below the
    /// block, and the block becomes a collapsible outline group — Excel's
    /// Data ▸ Subtotal.
    pub fn subtotal_range(&mut self, r0: usize, c0: usize, r1: usize, c1: usize) {
        if r1 <= r0 {
            return; // need at least two rows to subtotal
        }
        // Consecutive blocks of equal key value, top-down.
        let mut blocks: Vec<(usize, usize, String)> = Vec::new();
        let mut bs = r0;
        let mut key = self.cell_display_value(r0, c0);
        for r in (r0 + 1)..=r1 {
            let k = self.cell_display_value(r, c0);
            if k != key {
                blocks.push((bs, r - 1, key));
                bs = r;
                key = k;
            }
        }
        blocks.push((bs, r1, key));
        let mut off = 0usize;
        for (idx, (s0, e0, k)) in blocks.iter().enumerate() {
            let (s, e) = (s0 + off, e0 + off);
            self.insert_row(e + 1, 1);
            self.set_cell_text(e + 1, c0, &format!("{} Total", k));
            for c in (c0 + 1)..=c1 {
                let has_num =
                    (s..=e).any(|r| self.cell_raw_value(r, c).trim().parse::<f64>().is_ok());
                if has_num {
                    let col = string_at(c);
                    self.set_cell_text(
                        e + 1,
                        c,
                        &format!("=SUBTOTAL(9,{col}{}:{col}{})", s + 1, e + 1),
                    );
                }
            }
            self.add_row_group(s, e);
            off = idx + 1;
        }
    }

    /// Insert `n` blank columns at `at`, shifting existing cells/cols right.
    pub fn insert_col(&mut self, at: usize, n: usize) {
        self.mark_spills_dirty();
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
        shift_groups_for_insert(&mut self.col_groups, at, n);
        crate::core::table::shift_tables_for_insert(&mut self.tables, false, at, n);
    }

    /// Delete the column at `at`, shifting later cells/cols left.
    pub fn delete_col(&mut self, at: usize) {
        self.mark_spills_dirty();
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
        shift_groups_for_delete(&mut self.col_groups, at);
        crate::core::table::shift_tables_for_delete(&mut self.tables, false, at);
    }

    // --- Hide / unhide rows & columns (issue #14) ---

    /// Hide or reveal row `ri`.
    pub fn set_row_hidden(&mut self, ri: usize, hide: bool) {
        self.rows.entry(ri).or_default().set_hide(hide);
    }

    // --- Row/column outline groups (issue #30) ---

    /// Group rows `start..=end`. Invalid or duplicate ranges are ignored.
    pub fn add_row_group(&mut self, start: usize, end: usize) {
        if start > end
            || self
                .row_groups
                .iter()
                .any(|g| g.start == start && g.end == end)
        {
            return;
        }
        self.row_groups.push(OutlineGroup {
            start,
            end,
            collapsed: false,
        });
    }

    pub fn add_col_group(&mut self, start: usize, end: usize) {
        if start > end
            || self
                .col_groups
                .iter()
                .any(|g| g.start == start && g.end == end)
        {
            return;
        }
        self.col_groups.push(OutlineGroup {
            start,
            end,
            collapsed: false,
        });
    }

    /// Remove every row group that intersects `start..=end` (the Ungroup
    /// command), revealing any rows only those groups were hiding.
    pub fn remove_row_groups_overlapping(&mut self, start: usize, end: usize) {
        let keep = |g: &OutlineGroup| g.end < start || g.start > end;
        // Reveal the removed groups' members first — apply_row_groups only
        // visits rows of REMAINING groups, so without this a removed collapsed
        // group would leave its rows hidden forever.
        let removed: Vec<OutlineGroup> = self
            .row_groups
            .iter()
            .filter(|g| !keep(g))
            .cloned()
            .collect();
        self.row_groups.retain(keep);
        for g in &removed {
            for r in g.start..=g.end {
                self.set_row_hidden(r, false);
            }
        }
        // Re-hide anything a surviving collapsed group still covers.
        self.apply_row_groups();
    }

    pub fn remove_col_groups_overlapping(&mut self, start: usize, end: usize) {
        let keep = |g: &OutlineGroup| g.end < start || g.start > end;
        let removed: Vec<OutlineGroup> = self
            .col_groups
            .iter()
            .filter(|g| !keep(g))
            .cloned()
            .collect();
        self.col_groups.retain(keep);
        for g in &removed {
            for c in g.start..=g.end {
                self.set_col_hidden(c, false);
            }
        }
        self.apply_col_groups();
    }

    /// Collapse/expand row group `idx` (a gutter ± click).
    pub fn toggle_row_group(&mut self, idx: usize) {
        if let Some(g) = self.row_groups.get_mut(idx) {
            g.collapsed = !g.collapsed;
        }
        self.apply_row_groups();
    }

    pub fn toggle_col_group(&mut self, idx: usize) {
        if let Some(g) = self.col_groups.get_mut(idx) {
            g.collapsed = !g.collapsed;
        }
        self.apply_col_groups();
    }

    /// Outline level button `k` (issue #30): show levels `< k` expanded and
    /// collapse every group at level `>= k` — Excel's 1/2/3 buttons. The
    /// highest button (max level + 1) therefore expands everything.
    pub fn set_row_outline_level(&mut self, k: usize) {
        let levels = crate::core::outline::group_levels(&self.row_groups);
        for (g, lvl) in self.row_groups.iter_mut().zip(levels) {
            g.collapsed = lvl >= k;
        }
        self.apply_row_groups();
    }

    pub fn set_col_outline_level(&mut self, k: usize) {
        let levels = crate::core::outline::group_levels(&self.col_groups);
        for (g, lvl) in self.col_groups.iter_mut().zip(levels) {
            g.collapsed = lvl >= k;
        }
        self.apply_col_groups();
    }

    /// Recompute the hide flag for every row covered by a group: hidden iff
    /// ANY collapsed group contains it — so expanding an outer group leaves a
    /// still-collapsed inner group's rows hidden. Rows outside all groups
    /// (e.g. manually hidden, #14) are untouched.
    fn apply_row_groups(&mut self) {
        let groups = self.row_groups.clone();
        for r in groups
            .iter()
            .flat_map(|g| g.start..=g.end)
            .collect::<HashSet<_>>()
        {
            let hide = groups.iter().any(|g| g.collapsed && g.contains(r));
            self.set_row_hidden(r, hide);
        }
    }

    fn apply_col_groups(&mut self) {
        let groups = self.col_groups.clone();
        for c in groups
            .iter()
            .flat_map(|g| g.start..=g.end)
            .collect::<HashSet<_>>()
        {
            let hide = groups.iter().any(|g| g.collapsed && g.contains(c));
            self.set_col_hidden(c, hide);
        }
    }

    /// Whether row `ri` is currently hidden.
    pub fn is_row_hidden(&self, ri: usize) -> bool {
        self.rows.get(&ri).is_some_and(|r| r.hide)
    }

    /// Hide or reveal column `ci`.
    pub fn set_col_hidden(&mut self, ci: usize, hide: bool) {
        self.cols.set_hide(ci, hide);
    }

    /// Whether column `ci` is currently hidden.
    pub fn is_col_hidden(&self, ci: usize) -> bool {
        self.cols.data.get(&ci).is_some_and(|c| c.hide)
    }

    // --- Insert / delete cells with shift (issue #14) ---
    //
    // Relocated cells keep their content verbatim — formula references are not
    // rewritten (matching cut/paste, and sidestepping the ambiguous partial-
    // range adjustment that Excel itself warns can break formulas). Merges that
    // overlap the affected band are dropped.

    fn take_cell(&mut self, r: usize, c: usize) -> Option<Cell> {
        self.rows.get_mut(&r).and_then(|row| row.cells.remove(&c))
    }

    fn put_cell(&mut self, r: usize, c: usize, cell: Cell) {
        self.rows.entry(r).or_default().cells.insert(c, cell);
    }

    /// Insert a blank block over the rectangle (`r0,c0`)–(`r1,c1`), pushing the
    /// cells there (and beyond) right (`horizontal`) or down.
    pub fn insert_cells(&mut self, r0: usize, c0: usize, r1: usize, c1: usize, horizontal: bool) {
        let (r0, r1) = (r0.min(r1), r0.max(r1));
        let (c0, c1) = (c0.min(c1), c0.max(c1));
        if horizontal {
            let w = c1 - c0 + 1;
            for r in r0..=r1 {
                if let Some(row) = self.rows.get_mut(&r) {
                    // High → low so a moved cell never clobbers an unmoved one.
                    let mut cs: Vec<usize> =
                        row.cells.keys().copied().filter(|c| *c >= c0).collect();
                    cs.sort_unstable_by(|a, b| b.cmp(a));
                    for c in cs {
                        if let Some(cell) = row.cells.remove(&c) {
                            row.cells.insert(c + w, cell);
                        }
                    }
                }
            }
        } else {
            let h = r1 - r0 + 1;
            for c in c0..=c1 {
                let mut rs: Vec<usize> = self
                    .rows
                    .iter()
                    .filter(|(r, row)| **r >= r0 && row.cells.contains_key(&c))
                    .map(|(r, _)| *r)
                    .collect();
                rs.sort_unstable_by(|a, b| b.cmp(a)); // high → low
                for r in rs {
                    if let Some(cell) = self.take_cell(r, c) {
                        self.put_cell(r + h, c, cell);
                    }
                }
            }
        }
        self.merges
            .delete_intersecting(&CellRange::new(r0, c0, r1, c1));
    }

    /// Delete the rectangle (`r0,c0`)–(`r1,c1`), pulling the cells beyond it
    /// left (`horizontal`) or up.
    pub fn delete_cells(&mut self, r0: usize, c0: usize, r1: usize, c1: usize, horizontal: bool) {
        self.mark_spills_dirty();
        let (r0, r1) = (r0.min(r1), r0.max(r1));
        let (c0, c1) = (c0.min(c1), c0.max(c1));
        if horizontal {
            let w = c1 - c0 + 1;
            for r in r0..=r1 {
                if let Some(row) = self.rows.get_mut(&r) {
                    for c in c0..=c1 {
                        row.cells.remove(&c);
                    }
                    // Low → high so a moved cell never clobbers an unmoved one.
                    let mut cs: Vec<usize> =
                        row.cells.keys().copied().filter(|c| *c > c1).collect();
                    cs.sort_unstable();
                    for c in cs {
                        if let Some(cell) = row.cells.remove(&c) {
                            row.cells.insert(c - w, cell);
                        }
                    }
                }
            }
        } else {
            let h = r1 - r0 + 1;
            for c in c0..=c1 {
                for r in r0..=r1 {
                    self.take_cell(r, c);
                }
                let mut rs: Vec<usize> = self
                    .rows
                    .iter()
                    .filter(|(r, row)| **r > r1 && row.cells.contains_key(&c))
                    .map(|(r, _)| *r)
                    .collect();
                rs.sort_unstable(); // low → high
                for r in rs {
                    if let Some(cell) = self.take_cell(r, c) {
                        self.put_cell(r - h, c, cell);
                    }
                }
            }
        }
        self.merges
            .delete_intersecting(&CellRange::new(r0, c0, r1, c1));
        self.adjust_formulas_for_delete_cells(r0, c0, r1, c1, horizontal);
    }

    /// Rewrite cell references in every formula after a structural edit. Any
    /// reference whose row (or column, when `is_row` is false) index is
    /// `>= shift_from` is offset by `delta`.
    fn adjust_all_formulas(
        &mut self,
        is_row: bool,
        shift_from: usize,
        delta: isize,
        deleted: Option<usize>,
    ) {
        for row in self.rows.values_mut() {
            for cell in row.cells.values_mut() {
                if cell.text.starts_with('=') {
                    cell.text = adjust_formula_refs(&cell.text, is_row, shift_from, delta, deleted);
                }
            }
        }
    }

    /// After a rectangular cell deletion with shift (delete_cells), rewrite
    /// every formula: references inside the deleted rect become `#REF!`, and
    /// references to cells that shifted are adjusted.
    ///
    /// `horizontal=true` → cells in rows r0..=r1 to the right of c1 shift left;
    /// `horizontal=false` → cells in cols c0..=c1 below r1 shift up.
    fn adjust_formulas_for_delete_cells(
        &mut self,
        r0: usize,
        c0: usize,
        r1: usize,
        c1: usize,
        horizontal: bool,
    ) {
        for row in self.rows.values_mut() {
            for cell in row.cells.values_mut() {
                if cell.text.starts_with('=') {
                    cell.text =
                        adjust_refs_for_delete_cells(&cell.text, r0, c0, r1, c1, horizontal);
                }
            }
        }
    }

    /// On-screen row height: the stored (model) height × the view zoom
    /// (issue #32). A hidden row's 0 stays 0 at any zoom.
    pub fn get_row_height(&self, ri: usize) -> f64 {
        let h = self
            .rows
            .get(&ri)
            .map(|r| r.get_height())
            .unwrap_or(self.default_row_height);
        h * self.zoom
    }

    /// Set a row's MODEL height (unzoomed pixels). Callers translating a
    /// screen-pixel drag must divide by `zoom()` first (the renderer's
    /// clamped setters do).
    pub fn set_row_height(&mut self, ri: usize, height: f64) {
        let row = self.rows.entry(ri).or_default();
        row.set_height(height);
    }

    pub fn set_col_width(&mut self, ci: usize, width: f64) {
        self.cols.set_width(ci, width);
    }

    /// On-screen column width: stored (model) width × the view zoom (issue #32).
    pub fn get_col_width(&self, ci: usize) -> f64 {
        self.cols.get_width(ci) * self.zoom
    }

    /// Current view zoom factor (1.0 = 100%).
    pub fn zoom(&self) -> f64 {
        self.zoom
    }

    /// Set the view zoom, clamped to Excel's 10–400% range.
    pub fn set_zoom(&mut self, zoom: f64) {
        self.zoom = zoom.clamp(0.1, 4.0);
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
        // Through get_col_width (not cols.sum_width) so the zoom factor
        // (issue #32) applies here exactly as in every other geometry path.
        (0..self.freeze.1).map(|i| self.get_col_width(i)).sum()
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
            // Clearing the filter reveals every data row it may have hidden.
            if let Some(range) = self.auto_filter.range() {
                for ri in (range.sri + 1)..=range.eri {
                    self.set_row_hidden(ri, false);
                }
            }
            self.auto_filter.clear();
        } else {
            self.auto_filter.ref_ = Some(self.selector.range.to_string());
        }
    }

    /// The bounding (max_row, max_col) over all non-empty cells, if any —
    /// used to expand a single-cell selection to the whole table when the
    /// filter toggles on (issue #10).
    pub fn used_extent(&self) -> Option<(usize, usize)> {
        let mut out: Option<(usize, usize)> = None;
        for (ri, row) in &self.rows {
            for (ci, cell) in &row.cells {
                if !cell.text.is_empty() {
                    let (mr, mc) = out.unwrap_or((0, 0));
                    out = Some((mr.max(*ri), mc.max(*ci)));
                }
            }
        }
        // Spilled cells hold no text but display values — a trailing spill
        // extends the extent so CSV export / print include it (issue #33).
        for range in self.spill_ranges() {
            let (mr, mc) = out.unwrap_or((0, 0));
            out = Some((mr.max(range.eri), mc.max(range.eci)));
        }
        out
    }

    /// The rightmost non-empty column in row `ri`, or 0 if the row is empty.
    /// Used by the End key to jump to the last filled cell in the row (#41).
    pub fn row_last_filled_col(&self, ri: usize) -> usize {
        self.rows
            .get(&ri)
            .and_then(|row| {
                row.cells
                    .iter()
                    .filter(|(_, c)| !c.text.is_empty())
                    .map(|(ci, _)| *ci)
                    .max()
            })
            .unwrap_or(0)
    }

    /// Re-evaluate the active filters and hide/reveal the data rows of the
    /// autofilter range accordingly (issue #10). Filters match on the
    /// *displayed* value (formula results, applied formats) — the same string
    /// the filter dropdown lists. Rows outside the range are untouched.
    pub fn apply_filter_visibility(&mut self) {
        let af = self.auto_filter.clone();
        let (hidden, visible) = af.filtered_rows(|ri, ci| Some(self.cell_display_value(ri, ci)));
        for ri in &visible {
            self.set_row_hidden(*ri, false);
        }
        for ri in &hidden {
            self.set_row_hidden(*ri, true);
        }
    }

    /// Sort the autofilter range's data rows (header row excluded) by column
    /// `ci`, moving only the cells within the range's column span so data
    /// outside the table stays put (issue #10). Keys are displayed values:
    /// numbers compare numerically, text case-insensitively, blanks last.
    /// Sort rows within the autofilter range by a single column key.
    pub fn sort_filter_range(&mut self, ci: usize, asc: bool) {
        self.mark_spills_dirty();
        let Some(range) = self.auto_filter.range() else {
            return;
        };
        let (sri, eri) = (range.sri + 1, range.eri);
        if sri > eri {
            return;
        }
        let (sci, eci) = (range.sci, range.eci);
        let mut entries: Vec<(String, HashMap<usize, Cell>)> = (sri..=eri)
            .map(|ri| {
                let key = self.cell_display_value(ri, ci);
                let cells = self
                    .rows
                    .get(&ri)
                    .map(|row| {
                        row.cells
                            .iter()
                            .filter(|(c, _)| **c >= sci && **c <= eci)
                            .map(|(c, cell)| (*c, cell.clone()))
                            .collect()
                    })
                    .unwrap_or_default();
                (key, cells)
            })
            .collect();
        entries.sort_by(|a, b| cmp_cell_values(&a.0, &b.0, asc));
        for (offset, (_, cells)) in entries.into_iter().enumerate() {
            let row = self.rows.entry(sri + offset).or_default();
            row.cells.retain(|c, _| *c < sci || *c > eci);
            for (c, cell) in cells {
                row.cells.insert(c, cell);
            }
        }
        self.auto_filter
            .set_sorts(vec![Sort::new(ci, if asc { "asc" } else { "desc" })]);
        // Rows moved, so the hidden/visible assignment must be recomputed.
        self.apply_filter_visibility();
    }

    /// Sort rows within the autofilter range by multiple column keys (issue
    /// #14). Each `Sort` gives a column index and direction. The least
    /// significant key is applied first via stable sort so ties in later
    /// (more significant) keys preserve the earlier ordering.
    pub fn sort_filter_range_multi(&mut self, sorts: &[Sort]) {
        self.mark_spills_dirty();
        if sorts.is_empty() {
            return;
        }
        let Some(range) = self.auto_filter.range() else {
            return;
        };
        let (sri, eri) = (range.sri + 1, range.eri);
        if sri > eri {
            return;
        }
        let (sci, eci) = (range.sci, range.eci);
        let mut entries: Vec<(Vec<String>, HashMap<usize, Cell>)> = (sri..=eri)
            .map(|ri| {
                let keys: Vec<String> = sorts
                    .iter()
                    .map(|s| self.cell_display_value(ri, s.ci))
                    .collect();
                let cells = self
                    .rows
                    .get(&ri)
                    .map(|row| {
                        row.cells
                            .iter()
                            .filter(|(c, _)| **c >= sci && **c <= eci)
                            .map(|(c, cell)| (*c, cell.clone()))
                            .collect()
                    })
                    .unwrap_or_default();
                (keys, cells)
            })
            .collect();
        // Stable sort by each key least-significant-first.
        for (idx, sort) in sorts.iter().enumerate().rev() {
            let asc = sort.asc();
            entries.sort_by(|a, b| cmp_cell_values(&a.0[idx], &b.0[idx], asc));
        }
        for (offset, (_, cells)) in entries.into_iter().enumerate() {
            let row = self.rows.entry(sri + offset).or_default();
            row.cells.retain(|c, _| *c < sci || *c > eci);
            for (c, cell) in cells {
                row.cells.insert(c, cell);
            }
        }
        self.auto_filter.set_sorts(sorts.to_vec());
        self.apply_filter_visibility();
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

    // Dedup key for `add_style`. EVERY `Style` field must be compared here:
    // any omitted field makes two visually-distinct styles collapse into one,
    // silently discarding that attribute (this is how the borders bug shipped —
    // `border` was missing). When adding a field to `Style`, add it here too.
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
            && a.border == b.border
    }

    pub fn merge(&mut self) {
        if self.is_single_selected() {
            return;
        }
        self.merge_range(self.selector.range.clone());
    }

    /// Merge `range` into a single cell: its top-left becomes the anchor
    /// (carrying the `(extra_rows, extra_cols)` span) and the cells it covers
    /// are cleared. A 1×1 range is a no-op. Used both by the interactive merge
    /// (the current selection) and by clipboard paste re-applying merges from
    /// pasted HTML.
    pub fn merge_range(&mut self, range: CellRange) {
        self.mark_spills_dirty();
        let rn = range.eri.saturating_sub(range.sri) + 1;
        let cn = range.eci.saturating_sub(range.sci) + 1;
        if rn <= 1 && cn <= 1 {
            return;
        }
        let (sri, sci) = (range.sri, range.sci);
        let cell = self.get_cell_or_new(sri, sci);
        cell.merge = Some((rn - 1, cn - 1));
        self.merges.add(range.clone());
        for ri in range.sri..=range.eri {
            for ci in range.sci..=range.eci {
                if ri != sri || ci != sci {
                    if let Some(row) = self.rows.get_mut(&ri) {
                        row.delete_cell(ci);
                    }
                }
            }
        }
    }

    pub fn unmerge(&mut self) {
        self.mark_spills_dirty();
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

    /// Remove every merge that intersects `range`, clearing each affected
    /// anchor's per-cell `merge` marker too. Used by clipboard paste, which
    /// overwrites a region and must not leave a merge straddling it — pasting
    /// over any part of a merge unmerges the whole thing (matching Excel).
    /// Unlike `Merges::add`'s `delete_within`, this also drops merges that only
    /// partially overlap.
    pub fn unmerge_intersecting(&mut self, range: &CellRange) {
        self.mark_spills_dirty();
        let anchors: Vec<(usize, usize)> = {
            let mut v = Vec::new();
            self.merges.for_each(|m| {
                if m.intersects(range) {
                    v.push((m.sri, m.sci));
                }
            });
            v
        };
        for (ar, ac) in anchors {
            if let Some(cell) = self.get_cell_mut(ar, ac) {
                cell.merge = None;
            }
        }
        self.merges.delete_intersecting(range);
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
            "condfmts": serde_json::to_value(&self.cond_formats).unwrap_or_default(),
            // Outline groups (issue #30); collapse-hidden state rides with rows/cols.
            "rowGroups": serde_json::to_value(&self.row_groups).unwrap_or_default(),
            "colGroups": serde_json::to_value(&self.col_groups).unwrap_or_default(),
            "charts": serde_json::to_value(&self.charts).unwrap_or_default(),
            // PivotTable specs (issue #35). The materialised cells live on
            // a separate `DataProxy` in the workbook's `SheetsRegistry`;
            // this list is the *recipe* on the source sheet, so Refresh
            // can find it after a workbook round-trip.
            "pivots": serde_json::to_value(&self.pivots).unwrap_or_default(),
            // Slicers (issue #61) — visual filters that apply to every
            // pivot on this source sheet. Round-trips through
            // `get_data` / `set_data`; pre-#61 workbooks omit the key
            // and load with an empty list.
            "slicers": serde_json::to_value(&self.slicers).unwrap_or_default(),
            // Inline sparklines (Phase 4.1); absent in pre-4.1 workbooks.
            "sparklines": serde_json::to_value(&self.sparklines).unwrap_or_default(),
            // Floating images (Phase 4.2); absent in pre-4.2 workbooks.
            "images": serde_json::to_value(&self.images).unwrap_or_default(),
            // Sheet protection metadata (Phase 1.3).
            "protection": serde_json::to_value(&self.protection).unwrap_or_default(),
            // Excel-style tables (issue #34).
            "tables": serde_json::to_value(&self.tables).unwrap_or_default(),
            "page": serde_json::to_value(&self.page_setup).unwrap_or_default(),
            // Active cell (ri, ci) + selection rectangle, so a host can
            // round-trip selection state through get_data/load_data and read
            // the active cell from an on_change payload (issue #44).
            "sel": {
                "ri": self.selector.ri,
                "ci": self.selector.ci,
                "range": self.selector.range.to_string(),
            },
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
        if let Some(styles) = data
            .get("styles")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
        {
            self.styles = styles;
        }
        if let Some(merges) = data.get("merges").and_then(|v| v.as_array()) {
            let merge_strings: Vec<String> = merges
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect();
            self.merges.set_data(merge_strings);
        }
        if let Some(rows_data) = data.get("rows").and_then(|v| v.as_object()) {
            if let Some(len) = rows_data.get("len").and_then(|v| v.as_u64()) {
                self.row_count = len as usize;
            }
            if let Some(rows_obj) = rows_data
                .get("_")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
            {
                self.rows = rows_obj;
            }
        }
        if let Some(cols_data) = data.get("cols").and_then(|v| v.as_object()) {
            if let Some(len) = cols_data.get("len").and_then(|v| v.as_u64()) {
                self.cols.len = len as usize;
            }
            if let Some(cols_obj) = cols_data
                .get("_")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
            {
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
            .and_then(|v| serde_json::from_value::<HashMap<String, String>>(v.clone()).ok())
        {
            // Upper-case the keys so JSON-loaded ranges resolve the same as
            // name-box ones — resolve_name/get_named_range look up by the
            // upper-cased name, and set_named_range stores upper-cased keys.
            // Without this, a key like "testRange" never matches "TESTRANGE"
            // (issue #45).
            self.named_ranges = nr.into_iter().map(|(k, v)| (k.to_uppercase(), v)).collect();
        }
        if let Some(cf) = data
            .get("condfmts")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
        {
            self.cond_formats = cf;
        }
        if let Some(rg) = data
            .get("rowGroups")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
        {
            self.row_groups = rg;
        }
        if let Some(cg) = data
            .get("colGroups")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
        {
            self.col_groups = cg;
        }
        if let Some(ch) = data
            .get("charts")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
        {
            self.charts = ch;
        }
        if let Some(pv) = data.get("pivots").and_then(|v| {
            serde_json::from_value::<Vec<crate::core::pivot::PivotTable>>(v.clone()).ok()
        }) {
            // PivotTable specs on this source sheet (issue #35). The
            // materialised cells live on the output sheet; this list is
            // the recipe that `refresh_active_pivot` reads to re-run.
            self.pivots = pv;
        }
        if let Some(sl) = data
            .get("slicers")
            .and_then(|v| serde_json::from_value::<Vec<crate::core::pivot::Slicer>>(v.clone()).ok())
        {
            // Slicers (issue #61) — visual filters on the source sheet.
            // Absent in pre-#61 workbooks; `slicers` stays empty.
            self.slicers = sl;
        }
        if let Some(pr) = data
            .get("protection")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
        {
            // Sheet protection metadata (Phase 1.3). Absent in
            // pre-1.3 workbooks; `protection` stays at its default.
            // Mirror `enabled` onto the existing read-only flag so the
            // data-layer block stays consistent.
            self.protection = pr;
            self.set_read_only(self.protection.enabled);
        }
        if let Some(sp) = data
            .get("sparklines")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
        {
            // Sparklines (Phase 4.1). Absent in pre-4.1 workbooks;
            // `sparklines` stays at its default (empty).
            self.sparklines = sp;
        }
        if let Some(imgs) = data
            .get("images")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
        {
            // Floating images (Phase 4.2). Absent in pre-4.2
            // workbooks; `images` stays at its default (empty).
            self.images = imgs;
        }
        if let Some(imgs) = data
            .get("images")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
        {
            // Floating images (Phase 4.2). Absent in pre-4.2
            // workbooks; `images` stays at its default (empty).
            self.images = imgs;
        }
        if let Some(ts) = data
            .get("tables")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
        {
            self.tables = ts;
        }
        if let Some(ps) = data
            .get("page")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
        {
            self.page_setup = ps;
        }
        // Restore the active cell + selection range (issue #44). Absent `sel`
        // (older payloads) leaves the default A1 selection.
        if let Some(sel) = data.get("sel") {
            if let Some(ri) = sel.get("ri").and_then(|v| v.as_u64()) {
                self.selector.ri = ri as usize;
            }
            if let Some(ci) = sel.get("ci").and_then(|v| v.as_u64()) {
                self.selector.ci = ci as usize;
            }
            if let Some(range) = sel.get("range").and_then(|v| v.as_str()) {
                if let Ok(r) = CellRange::from_str(range) {
                    self.selector.range = r;
                }
            }
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
    let (masked, mut placeholders) = mask_sheet_prefixes(text);
    // Structured refs are name-based and never shift (issue #34).
    let masked = mask_struct_refs(&masked, &mut placeholders);
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

            format!(
                "{}{}{}{}",
                col_lock,
                string_at(new_col),
                row_lock,
                new_row + 1
            )
        })
        .to_string();
    restore_placeholders(&shifted, &placeholders)
}

/// After a rectangular cell deletion with shift, rewrite cell references:
/// those inside the deleted rectangle become `#REF!`; those that shifted
/// are adjusted. The shift logic mirrors the cell-move in `delete_cells`.
fn adjust_refs_for_delete_cells(
    text: &str,
    r0: usize,
    c0: usize,
    r1: usize,
    c1: usize,
    horizontal: bool,
) -> String {
    let (masked, mut placeholders) = mask_sheet_prefixes(text);
    let masked = mask_struct_refs(&masked, &mut placeholders);
    let w = c1 - c0 + 1;
    let h = r1 - r0 + 1;
    let re = Regex::new(r"(\$?)([A-Za-z]+)(\$?)([0-9]+)").unwrap();
    let shifted = re
        .replace_all(&masked, |caps: &regex::Captures| {
            let col_lock = &caps[1];
            let row_lock = &caps[3];
            let col = index_at(&caps[2]);
            let row = caps[4].parse::<usize>().unwrap_or(1).saturating_sub(1);

            // References inside the deleted rectangle → #REF!.
            if row >= r0 && row <= r1 && col >= c0 && col <= c1 {
                return "#REF!".to_string();
            }

            let mut new_col = col;
            let mut new_row = row;
            if horizontal {
                // Cells in the same row band, right of the rect, shift left.
                if col_lock.is_empty() && row >= r0 && row <= r1 && col > c1 {
                    new_col = col.saturating_sub(w);
                }
            } else {
                // Cells in the same column band, below the rect, shift up.
                if row_lock.is_empty() && col >= c0 && col <= c1 && row > r1 {
                    new_row = row.saturating_sub(h);
                }
            }

            format!(
                "{}{}{}{}",
                col_lock,
                string_at(new_col),
                row_lock,
                new_row + 1
            )
        })
        .to_string();
    restore_placeholders(&shifted, &placeholders)
}

/// Shift every *relative* component of a formula's cell references by
/// (`drow`, `dcol`) — the copy/fill transform. `$`-anchored components stay
/// put. Sheet prefixes are masked out so the regex only touches refs
/// (issue #4).
/// Order two cell display values for sorting (issue #10): numbers compare
/// numerically, text case-insensitively, and blanks always sort last
/// regardless of direction (matching Excel).
pub(crate) fn cmp_cell_values(a: &str, b: &str, asc: bool) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let (ea, eb) = (a.trim().is_empty(), b.trim().is_empty());
    match (ea, eb) {
        (true, true) => return Ordering::Equal,
        (true, false) => return Ordering::Greater,
        (false, true) => return Ordering::Less,
        (false, false) => {}
    }
    let ord = match (a.trim().parse::<f64>(), b.trim().parse::<f64>()) {
        (Ok(x), Ok(y)) => x.partial_cmp(&y).unwrap_or(Ordering::Equal),
        _ => a.to_lowercase().cmp(&b.to_lowercase()),
    };
    if asc {
        ord
    } else {
        ord.reverse()
    }
}

fn shift_formula_refs(text: &str, drow: isize, dcol: isize) -> String {
    let (masked, mut placeholders) = mask_sheet_prefixes(text);
    // Structured refs are name-based: copy/fill leaves them as-is, which is
    // exactly Excel's behavior for `Table1[Col]` and `[@Col]` (issue #34).
    let masked = mask_struct_refs(&masked, &mut placeholders);
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
            format!(
                "{}{}{}{}",
                col_lock,
                string_at(new_col),
                row_lock,
                new_row + 1
            )
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

/// Mask structured table references (issue #34) — `Sales[Amount]`,
/// `T1[[#Totals],[Amt]]`, `[@Col]` — with the same placeholder scheme as
/// `mask_sheet_prefixes`, so the cell-ref regex can't mangle them: a table
/// name like `Table1` is letters+digits, exactly cell-ref-shaped, and the
/// column names inside the brackets may be too.
fn mask_struct_refs(text: &str, placeholders: &mut Vec<(String, String)>) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '[' {
            // The span includes the identifier directly before the bracket
            // (absent for the bare `[@Col]` form). `\u{0001}` delimits
            // already-masked placeholders, so the walk can't eat into one.
            let mut start = out.len();
            while let Some(prev) = out[..start].chars().next_back() {
                if prev.is_alphanumeric() || prev == '_' {
                    start -= prev.len_utf8();
                } else {
                    break;
                }
            }
            // Find the matching close bracket (specs nest one level).
            let mut depth = 0usize;
            let mut end = None;
            for (j, &ch) in chars.iter().enumerate().skip(i) {
                match ch {
                    '[' => depth += 1,
                    ']' => {
                        depth -= 1;
                        if depth == 0 {
                            end = Some(j);
                            break;
                        }
                    }
                    _ => {}
                }
            }
            if let Some(end) = end {
                let mut span = out[start..].to_string();
                out.truncate(start);
                span.extend(&chars[i..=end]);
                let key = format!("\u{0001}#{}#\u{0001}", placeholders.len());
                placeholders.push((span, key.clone()));
                out.push_str(&key);
                i = end + 1;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
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
        let nums: Option<Vec<f64>> = source
            .iter()
            .map(|s| s.trim().parse::<f64>().ok())
            .collect();
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

/// Shift outline groups for `n` tracks inserted at `at` (issue #30): members
/// at/after the insertion move down, so a group straddling `at` grows.
fn shift_groups_for_insert(groups: &mut [OutlineGroup], at: usize, n: usize) {
    for g in groups.iter_mut() {
        if g.start >= at {
            g.start += n;
        }
        if g.end >= at {
            g.end += n;
        }
    }
}

/// Shift outline groups for the track deleted at `at`: a group containing it
/// shrinks; a group reduced to nothing is dropped.
fn shift_groups_for_delete(groups: &mut Vec<OutlineGroup>, at: usize) {
    groups.retain_mut(|g| {
        if g.start > at {
            g.start -= 1;
        }
        if g.end >= at {
            if g.end == 0 {
                return false; // the single-track group at row/col 0 was deleted
            }
            g.end -= 1;
        }
        g.start <= g.end
    });
}

/// Validate and parse an A1 reference string — `"A1"` or `"A1:B3"` — into
/// bounds `(r0, c0, r1, c1)`. `None` for anything that isn't a well-formed
/// reference, so `INDIRECT` of garbage yields `#REF!` (issue #37).
fn parse_a1_ref(s: &str) -> Option<(usize, usize, usize, usize)> {
    let s = s.trim();
    let valid = |part: &str| crate::formula::parser::looks_like_cell_ref(part.trim());
    match s.split_once(':') {
        Some((a, b)) => {
            if !valid(a) || !valid(b) {
                return None;
            }
            let (c0, r0) = exp2xy(a.trim());
            let (c1, r1) = exp2xy(b.trim());
            Some((r0.min(r1), c0.min(c1), r0.max(r1), c0.max(c1)))
        }
        None => {
            if !valid(s) {
                return None;
            }
            let (c, r) = exp2xy(s);
            Some((r, c, r, c))
        }
    }
}

/// Build an Excel A1 address string for 1-based `(row, col)`. `abs`: 1=`$A$1`,
/// 2=`A$1` (abs row), 3=`$A1` (abs col), 4=`A1` (issue #37).
fn format_address(row: usize, col: usize, abs: i64) -> String {
    let col_abs = matches!(abs, 1 | 3);
    let row_abs = matches!(abs, 1 | 2);
    format!(
        "{}{}{}{}",
        if col_abs { "$" } else { "" },
        string_at(col - 1),
        if row_abs { "$" } else { "" },
        row
    )
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

/// Display string for a computed formula value, shared by the normal and
/// spilled render paths: a blank shows 0, matching Excel's `=A1` on an empty
/// cell (issue #36); an array shows its top-left (issue #33).
fn value_display(v: &Value) -> String {
    match v {
        Value::Number(n) => format_number(*n),
        Value::Text(s) => s.clone(),
        Value::Blank => format_number(0.0),
        // Per-cell errors render as their Excel code so a spill like
        // `=A1:A3/0` shows `#DIV/0!` in every spilled cell (issue #56).
        Value::Error(e) => e.code().to_string(),
        Value::Array(_) => value_display(&v.top_left()),
        Value::Lambda { .. } => "[LAMBDA]".to_string(),
    }
}

/// Cheap pre-filter for spill anchors (issue #33): only the dynamic-array
/// functions — or a range in expression position (`=A1:B3`, `=A1:A9*2`),
/// hence the `:` check — can put a `Value::Array` at a formula's top level.
/// Over-matching is fine (`=SUM(A1:B3)` evaluates to a scalar and is simply
/// not an anchor); the filter only avoids evaluating every formula twice.
fn is_spill_candidate(text: &str) -> bool {
    // `:` catches expression-position ranges; `[` catches structured table
    // references like `=Sales[Amount]` (issue #34), which are also arrays.
    if text.contains(':') || text.contains('[') {
        return true;
    }
    let up = text.to_uppercase();
    [
        "FILTER(",
        "SORT(",
        "SORTBY(",
        "UNIQUE(",
        "SEQUENCE(",
        "RANDARRAY(",
    ]
    .iter()
    .any(|f| up.contains(f))
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
    /// An array function produced an empty result (Excel's #CALC!), e.g.
    /// `FILTER` with no matches and no `if_empty` (issue #33).
    Calc,
    /// A dynamic-array result couldn't spill because its target range is
    /// obstructed or out of bounds (issue #33).
    Spill,
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
            EvalErr::Calc => "#CALC!",
            EvalErr::Spill => "#SPILL!",
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
            "#CALC!" => Some(EvalErr::Calc),
            "#SPILL!" => Some(EvalErr::Spill),
            _ => None,
        }
    }
}

/// A formula value (issue #2): numbers stay `f64`; text survives the trip
/// through the evaluator so string functions and comparisons can see it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Value {
    Number(f64),
    Text(String),
    /// An empty cell. Coerces to `0` / `""` like before, but is distinguishable
    /// by `ISBLANK` / `COUNTA` / `COUNTBLANK` (issue #36).
    Blank,
    /// A per-cell error (`#DIV/0!`, `#VALUE!`, …). Spilled into neighbouring
    /// cells just like any other `Value` so a formula like `=A1:A3/0`
    /// produces three #DIV/0! cells instead of collapsing to a single
    /// #DIV/0! at the anchor (issue #56).
    Error(EvalErr),
    /// A dynamic-array result (row-major, rectangular): FILTER/SORT/UNIQUE/
    /// SEQUENCE/… (issue #33). At a formula's top level it spills into
    /// neighboring cells; in scalar context it collapses to its top-left.
    Array(Vec<Vec<Value>>),
    /// A LAMBDA value (Phase 3.3). Captures the parameter names and
    /// the body token stream; evaluating the body defers until the
    /// lambda is invoked (typically through MAP/REDUCE/BYROW).
    /// Stored as plain owned tokens so a Lambda can outlive the
    /// surrounding parser span.
    Lambda {
        params: Vec<String>,
        body_tokens: Vec<Token>,
    },
}

impl Value {
    /// Scalar view of an array: its top-left element (Excel's implicit
    /// intersection-free collapse for our scalar contexts). Blank when empty.
    fn top_left(&self) -> Value {
        match self {
            Value::Array(g) => g
                .first()
                .and_then(|r| r.first())
                .cloned()
                .unwrap_or(Value::Blank),
            v => v.clone(),
        }
    }

    /// Numeric coercion, mirroring the engine's historic behavior: numeric
    /// text parses, date text becomes its serial, blanks and anything else 0.
    fn as_number(&self) -> f64 {
        match self {
            Value::Number(n) => *n,
            Value::Text(s) => {
                let t = s.trim();
                t.parse::<f64>()
                    .unwrap_or_else(|_| crate::core::date::parse_date(t).unwrap_or(0.0))
            }
            Value::Blank => 0.0,
            Value::Error(_) => 0.0,
            Value::Array(_) => self.top_left().as_number(),
            Value::Lambda { .. } => 0.0,
        }
    }

    /// Text coercion: numbers render the way the grid displays them.
    fn as_text(&self) -> String {
        match self {
            Value::Number(n) => format_number(*n),
            Value::Text(s) => s.clone(),
            Value::Blank => String::new(),
            // Error values render as their Excel code (issue #56).
            Value::Error(e) => e.code().to_string(),
            Value::Array(_) => self.top_left().as_text(),
            Value::Lambda { .. } => "[LAMBDA]".to_string(),
        }
    }

    fn is_truthy(&self) -> bool {
        self.as_number() != 0.0
    }
}

/// One function argument: a scalar or a row-major range grid (issue #2).
#[derive(Debug, Clone)]
enum Arg {
    Scalar(Value),
    Range(Vec<Vec<Value>>),
}

impl Arg {
    /// An evaluated expression as an argument: an array result keeps its
    /// shape as a `Range` so `=SUM(UNIQUE(A1:A9))` sees every element
    /// (issue #33); everything else is a scalar.
    fn from_value(v: Value) -> Arg {
        match v {
            Value::Array(rows) => Arg::Range(rows),
            v => Arg::Scalar(v),
        }
    }

    /// Scalar context: a range collapses to its top-left cell.
    fn into_scalar(self) -> Value {
        match self {
            Arg::Scalar(v) => v.top_left(),
            Arg::Range(rows) => rows
                .into_iter()
                .flatten()
                .next()
                .unwrap_or(Value::Number(0.0)),
        }
    }

    fn to_scalar(&self) -> Value {
        self.clone().into_scalar()
    }

    /// Row-major cells; a scalar is a 1×1 range.
    fn cells(&self) -> Vec<Value> {
        match self {
            Arg::Scalar(Value::Array(rows)) => rows.iter().flatten().cloned().collect(),
            Arg::Scalar(v) => vec![v.clone()],
            Arg::Range(rows) => rows.iter().flatten().cloned().collect(),
        }
    }

    /// The grid view (scalars are 1×1).
    fn grid(&self) -> Vec<Vec<Value>> {
        match self {
            Arg::Scalar(Value::Array(rows)) => rows.clone(),
            Arg::Scalar(v) => vec![vec![v.clone()]],
            Arg::Range(rows) => rows.clone(),
        }
    }
}

/// Compare two values Excel-style: numbers numerically, text
/// case-insensitively; a number never equals text (and orders before it).
/// `None` only for NaN comparisons, where every operator yields false.
fn compare_values(a: &Value, b: &Value) -> Option<std::cmp::Ordering> {
    use std::cmp::Ordering;
    // Arrays compare by their top-left scalar (issue #33).
    if matches!(a, Value::Array(_)) || matches!(b, Value::Array(_)) {
        return compare_values(&a.top_left(), &b.top_left());
    }
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.partial_cmp(y),
        (Value::Text(x), Value::Text(y)) => Some(x.to_lowercase().cmp(&y.to_lowercase())),
        (Value::Number(_), Value::Text(_)) => Some(Ordering::Less),
        (Value::Text(_), Value::Number(_)) => Some(Ordering::Greater),
        // A blank compares as 0, preserving the historic "empty == 0".
        (Value::Blank, _) => compare_values(&Value::Number(0.0), b),
        (_, Value::Blank) => compare_values(a, &Value::Number(0.0)),
        // An error sorts *after* everything else and is only equal to
        // itself (issue #56). Two equal errors are Equal; mixed errors
        // are Less (consistent with number-vs-text ordering).
        (Value::Error(x), Value::Error(y)) => {
            if x == y {
                Some(Ordering::Equal)
            } else {
                Some(Ordering::Less)
            }
        }
        (Value::Error(_), _) => Some(Ordering::Greater),
        (_, Value::Error(_)) => Some(Ordering::Less),
        // Unreachable: arrays were collapsed to scalars above.
        (Value::Array(_), _) | (_, Value::Array(_)) => None,
        // Lambda values are not directly comparable — treat them
        // as greater than every non-error value to keep the
        // compare_values total order sound.
        (Value::Lambda { .. }, _) | (_, Value::Lambda { .. }) => Some(Ordering::Greater),
    }
}

/// Apply a binary operation, broadcasting element-wise when either side is an
/// array (issue #33): same-shape arrays pair up element by element, a scalar
/// (or 1×1 array) pairs against every element, and mismatched shapes are
/// #VALUE!. Scalar ∘ scalar is just `f`.
fn broadcast2(
    l: &Value,
    r: &Value,
    f: &dyn Fn(&Value, &Value) -> Result<Value, EvalErr>,
) -> Result<Value, EvalErr> {
    match (l, r) {
        (Value::Array(a), Value::Array(b)) => {
            let (ar, ac) = (a.len(), a.first().map_or(0, Vec::len));
            let (br, bc) = (b.len(), b.first().map_or(0, Vec::len));
            if (ar, ac) == (1, 1) {
                return broadcast2(&a[0][0], r, f);
            }
            if (br, bc) == (1, 1) {
                return broadcast2(l, &b[0][0], f);
            }
            if (ar, ac) != (br, bc) {
                return Err(EvalErr::Value);
            }
            let mut out = Vec::with_capacity(ar);
            for (ra, rb) in a.iter().zip(b) {
                let mut row = Vec::with_capacity(ra.len());
                for (x, y) in ra.iter().zip(rb) {
                    // Per-cell errors stay per-cell: emit `Value::Error`
                    // so the spill renders the right code in every cell
                    // (issue #56). Whole-array errors (shape mismatch
                    // above) still propagate via `?`.
                    row.push(match f(x, y) {
                        Ok(v) => v,
                        Err(e) => Value::Error(e),
                    });
                }
                out.push(row);
            }
            Ok(Value::Array(out))
        }
        (Value::Array(a), s) => {
            let mut out = Vec::with_capacity(a.len());
            for ra in a {
                let mut row = Vec::with_capacity(ra.len());
                for x in ra {
                    row.push(match f(x, s) {
                        Ok(v) => v,
                        Err(e) => Value::Error(e),
                    });
                }
                out.push(row);
            }
            Ok(Value::Array(out))
        }
        (s, Value::Array(b)) => {
            let mut out = Vec::with_capacity(b.len());
            for rb in b {
                let mut row = Vec::with_capacity(rb.len());
                for y in rb {
                    row.push(match f(s, y) {
                        Ok(v) => v,
                        Err(e) => Value::Error(e),
                    });
                }
                out.push(row);
            }
            Ok(Value::Array(out))
        }
        (a, b) => f(a, b),
    }
}

/// Flatten arguments row-major (ranges expand; scalars pass through).
fn flatten_values(args: &[Arg]) -> Vec<Value> {
    args.iter().flat_map(|a| a.cells()).collect()
}

/// Build a reusable matcher from a SUMIF/COUNTIF-style criterion: a leading
/// comparator (`>= <= <> > < =`) compares numerically or as text; `*`/`?`
/// wildcards match text; anything else is (numeric or case-insensitive)
/// equality (issue #2).
fn criteria_matcher(criterion: &Value) -> Box<dyn Fn(&Value) -> bool> {
    use std::cmp::Ordering::*;
    let c = criterion.as_text();
    for op in [">=", "<=", "<>", ">", "<", "="] {
        if let Some(rest) = c.strip_prefix(op) {
            let rhs = rest
                .trim()
                .parse::<f64>()
                .map(Value::Number)
                .unwrap_or_else(|_| Value::Text(rest.trim().to_string()));
            let op = op.to_string();
            return Box::new(move |v| {
                let ord = compare_values(v, &rhs);
                match op.as_str() {
                    "=" => ord == Some(Equal),
                    "<>" => ord != Some(Equal),
                    ">" => ord == Some(Greater),
                    "<" => ord == Some(Less),
                    ">=" => matches!(ord, Some(Greater | Equal)),
                    "<=" => matches!(ord, Some(Less | Equal)),
                    _ => false,
                }
            });
        }
    }
    if c.contains('*') || c.contains('?') {
        let mut re = String::from("(?i)^");
        for ch in c.chars() {
            match ch {
                '*' => re.push_str(".*"),
                '?' => re.push('.'),
                _ => re.push_str(&regex::escape(&ch.to_string())),
            }
        }
        re.push('$');
        if let Ok(rx) = regex::Regex::new(&re) {
            return Box::new(move |v| rx.is_match(&v.as_text()));
        }
    }
    if let Ok(n) = c.trim().parse::<f64>() {
        return Box::new(move |v| compare_values(v, &Value::Number(n)) == Some(Equal));
    }
    Box::new(move |v| v.as_text().eq_ignore_ascii_case(&c))
}

/// VLOOKUP/HLOOKUP core: search the first column/row; exact when
/// `approx == false`, else the last key ≤ needle (assumes the keys are
/// sorted ascending, like Excel). Missing → #N/A, bad index → #REF!.
fn lookup(
    needle: &Value,
    grid: &[Vec<Value>],
    idx: usize,
    approx: bool,
    vertical: bool,
) -> Result<Value, EvalErr> {
    use std::cmp::Ordering::*;
    let n = if vertical {
        grid.len()
    } else {
        grid.first().map_or(0, Vec::len)
    };
    let key = |i: usize| -> Option<&Value> {
        if vertical {
            grid.get(i)?.first()
        } else {
            grid.first()?.get(i)
        }
    };
    let out = |i: usize| -> Result<Value, EvalErr> {
        let v = if vertical {
            grid.get(i).and_then(|r| r.get(idx))
        } else {
            grid.get(idx).and_then(|r| r.get(i))
        };
        v.cloned().ok_or(EvalErr::Ref)
    };
    let mut best: Option<usize> = None;
    for i in 0..n {
        let Some(k) = key(i) else { continue };
        match compare_values(k, needle) {
            Some(Equal) => return out(i),
            Some(Less) if approx => best = Some(i),
            _ => {}
        }
    }
    match best {
        Some(i) if approx => out(i),
        _ => Err(EvalErr::Na),
    }
}

/// MATCH core: 0 = exact, 1 = largest ≤ needle (ascending), -1 = smallest ≥
/// needle (descending). 1-based position; missing → #N/A.
fn match_position(needle: &Value, cells: &[Value], mode: f64) -> Result<Value, EvalErr> {
    use std::cmp::Ordering::*;
    let mut best: Option<usize> = None;
    for (i, v) in cells.iter().enumerate() {
        let ord = compare_values(v, needle);
        match mode as i64 {
            0 => {
                if ord == Some(Equal) {
                    return Ok(Value::Number((i + 1) as f64));
                }
            }
            -1 => match ord {
                Some(Equal) => return Ok(Value::Number((i + 1) as f64)),
                Some(Greater) => best = Some(i),
                _ => {}
            },
            _ => match ord {
                Some(Equal) => return Ok(Value::Number((i + 1) as f64)),
                Some(Less) => best = Some(i),
                _ => {}
            },
        }
    }
    best.map(|i| Value::Number((i + 1) as f64))
        .ok_or(EvalErr::Na)
}

/// Functions that must observe a *failed* or specifically-typed argument
/// rather than just its coerced value: `IFERROR`/`IFNA` swap in a fallback,
/// and the `IS*` predicates test a condition. They are handled here — before
/// arguments are unwrapped in `eval_expr` — because a computed error short-
/// circuits the normal argument path (issue #27). `None` means "not one of
/// these", so the regular dispatch takes over.
fn apply_info_function(
    name: &str,
    args: &[Result<Arg, EvalErr>],
) -> Option<Result<Value, EvalErr>> {
    // The error carried by an argument: a computed `Err`, or a literal error
    // value (`#REF!`, …) sitting in a referenced cell.
    let arg_error = |a: Option<&Result<Arg, EvalErr>>| -> Option<EvalErr> {
        match a {
            Some(Err(e)) => Some(*e),
            Some(Ok(arg)) => EvalErr::from_literal(&arg.to_scalar().as_text()),
            None => None,
        }
    };
    // Pass an argument through as a scalar (or propagate its error).
    let passthrough = |a: Option<&Result<Arg, EvalErr>>| -> Result<Value, EvalErr> {
        match a {
            Some(Ok(arg)) => Ok(arg.clone().into_scalar()),
            Some(Err(e)) => Err(*e),
            None => Ok(Value::Number(0.0)),
        }
    };
    let bool_v = |b: bool| Ok(Value::Number(if b { 1.0 } else { 0.0 }));
    let err0 = arg_error(args.first());
    // arg0's value when it isn't an error — for the type predicates. A blank
    // is its own kind: neither number nor text (issue #36).
    let scalar0 = || -> Option<Value> {
        match args.first() {
            Some(Ok(a)) if err0.is_none() => Some(a.to_scalar()),
            _ => None,
        }
    };

    let v = match name.to_uppercase().as_str() {
        "IFERROR" => {
            if err0.is_some() {
                passthrough(args.get(1))
            } else {
                passthrough(args.first())
            }
        }
        "IFNA" => {
            if err0 == Some(EvalErr::Na) {
                passthrough(args.get(1))
            } else {
                passthrough(args.first())
            }
        }
        "ISERROR" => bool_v(err0.is_some()),
        "ISNA" => bool_v(err0 == Some(EvalErr::Na)),
        "ISERR" => bool_v(matches!(err0, Some(e) if e != EvalErr::Na)),
        "ISNUMBER" => bool_v(matches!(scalar0(), Some(Value::Number(_)))),
        "ISTEXT" => bool_v(matches!(scalar0(), Some(Value::Text(_)))),
        "ISBLANK" => bool_v(matches!(scalar0(), Some(Value::Blank))),
        "TYPE" => {
            let t = match scalar0() {
                Some(Value::Number(_)) => 1.0,
                Some(Value::Text(_)) => 2.0,
                Some(Value::Blank) => 1.0, // blank cells type as number (0)
                Some(Value::Error(_)) => 16.0,
                Some(Value::Array(_)) => 64.0,
                // Lambda is treated as a "function" — Excel uses 64 for
                // arrays; we reuse that code so a future official
                // type-128 branch can replace it without breaking
                // existing workbooks.
                Some(Value::Lambda { .. }) => 64.0,
                None if err0.is_some() => 16.0,
                None => 1.0,
            };
            Ok(Value::Number(t))
        }
        "N" => {
            match scalar0() {
                Some(Value::Number(n)) => Ok(Value::Number(n)),
                Some(Value::Text(_)) => {
                    // N of text is 0 unless it's a date string
                    Ok(Value::Number(0.0))
                }
                Some(Value::Blank) => Ok(Value::Number(0.0)),
                Some(Value::Error(e)) => Err(e),
                Some(Value::Array(_)) => {
                    // N(array) returns N of first element
                    Ok(Value::Number(0.0))
                }
                Some(Value::Lambda { .. }) => Ok(Value::Number(0.0)),
                None if err0.is_some() => Err(err0.unwrap()),
                None => Ok(Value::Number(0.0)),
            }
        }
        "T" => match scalar0() {
            Some(Value::Text(t)) => Ok(Value::Text(t)),
            _ => Ok(Value::Text(String::new())),
        },
        _ => return None,
    };

    // Guard: NaN/∞ from number ops becomes #NUM!.
    if matches!(name.to_uppercase().as_str(), "N")
        && matches!(&v, Ok(Value::Number(n)) if !n.is_finite())
    {
        return Some(Err(EvalErr::Num));
    }
    Some(v)
}

/// Text, criteria, and lookup functions (issue #2) — the ones that need real
/// values or range shape. `None` means "not one of these" and the flattened
/// numeric catalog below takes over.
fn apply_special_function(upper: &str, args: &[Arg]) -> Result<Option<Value>, EvalErr> {
    let scalar = |i: usize| -> Value {
        args.get(i)
            .map(|a| a.to_scalar())
            .unwrap_or(Value::Number(0.0))
    };
    let text = |i: usize| scalar(i).as_text();
    let num = |i: usize| scalar(i).as_number();

    let v = match upper {
        "CONCAT" | "CONCATENATE" => {
            Value::Text(flatten_values(args).iter().map(Value::as_text).collect())
        }
        "LEN" => Value::Number(text(0).chars().count() as f64),
        "UPPER" => Value::Text(text(0).to_uppercase()),
        "LOWER" => Value::Text(text(0).to_lowercase()),
        "TRIM" => Value::Text(text(0).trim().to_string()),
        "LEFT" => {
            let n = if args.len() >= 2 {
                num(1).max(0.0) as usize
            } else {
                1
            };
            Value::Text(text(0).chars().take(n).collect())
        }
        "RIGHT" => {
            let s: Vec<char> = text(0).chars().collect();
            let n = if args.len() >= 2 {
                num(1).max(0.0) as usize
            } else {
                1
            };
            Value::Text(s[s.len().saturating_sub(n)..].iter().collect())
        }
        "MID" => {
            if args.len() < 3 {
                return Err(EvalErr::Value);
            }
            let start = (num(1).max(1.0) as usize).saturating_sub(1);
            let n = num(2).max(0.0) as usize;
            Value::Text(text(0).chars().skip(start).take(n).collect())
        }
        "TEXT" => {
            if args.len() < 2 {
                return Err(EvalErr::Value);
            }
            Value::Text(crate::core::format::format_value(&text(0), &text(1)))
        }
        "COUNTIF" => {
            if args.len() < 2 {
                return Err(EvalErr::Value);
            }
            let matches = criteria_matcher(&scalar(1));
            let mut n = 0usize;
            for v in args[0].cells().iter() {
                if matches(v) {
                    n += 1;
                }
            }
            Value::Number(n as f64)
        }
        "SUMIF" | "AVERAGEIF" => {
            if args.len() < 2 {
                return Err(EvalErr::Value);
            }
            let matches = criteria_matcher(&scalar(1));
            let test = args[0].cells();
            // The optional third argument is the range actually summed,
            // paired positionally with the tested one (Excel semantics).
            let pool = if args.len() >= 3 {
                args[2].cells()
            } else {
                test.clone()
            };
            let mut sum = 0.0;
            let mut n = 0usize;
            for (i, probe) in test.iter().enumerate() {
                if matches(probe) {
                    sum += pool.get(i).map(Value::as_number).unwrap_or(0.0);
                    n += 1;
                }
            }
            if upper == "SUMIF" {
                Value::Number(sum)
            } else if n == 0 {
                return Err(EvalErr::Div0);
            } else {
                Value::Number(sum / n as f64)
            }
        }
        "VLOOKUP" | "HLOOKUP" => {
            if args.len() < 3 {
                return Err(EvalErr::Value);
            }
            let needle = scalar(0);
            let grid = args[1].grid();
            let idx = (num(2) as usize).saturating_sub(1);
            let approx = if args.len() >= 4 {
                scalar(3).is_truthy()
            } else {
                true
            };
            return lookup(&needle, &grid, idx, approx, upper == "VLOOKUP").map(Some);
        }
        "INDEX" => {
            if args.len() < 2 {
                return Err(EvalErr::Value);
            }
            let grid = args[0].grid();
            let (rows, cols) = (grid.len(), grid.first().map_or(0, Vec::len));
            let a = num(1) as usize; // 1-based
            let (r, c) = if args.len() >= 3 {
                (a, num(2) as usize)
            } else if rows == 1 {
                (1, a)
            } else if cols == 1 {
                (a, 1)
            } else {
                return Err(EvalErr::Ref);
            };
            if r == 0 || c == 0 || r > rows || c > cols {
                return Err(EvalErr::Ref);
            }
            grid[r - 1][c - 1].clone()
        }
        "MATCH" => {
            if args.len() < 2 {
                return Err(EvalErr::Value);
            }
            let needle = scalar(0);
            let cells = args[1].cells(); // a single row or column flattens cleanly
            let mode = if args.len() >= 3 { num(2) } else { 1.0 };
            return match_position(&needle, &cells, mode).map(Some);
        }

        // Multi-criteria aggregates. COUNTIFS is all (range, criterion) pairs;
        // SUMIFS/AVERAGEIFS reserve arg 0 for the summed range, pairs from 1.
        "COUNTIFS" | "SUMIFS" | "AVERAGEIFS" => {
            let pairs_from = if upper == "COUNTIFS" { 0 } else { 1 };
            if args.len() <= pairs_from || !(args.len() - pairs_from).is_multiple_of(2) {
                return Err(EvalErr::Value);
            }
            type Criterion = Box<dyn Fn(&Value) -> bool>;
            let pairs: Vec<(Vec<Value>, Criterion)> = (pairs_from..args.len())
                .step_by(2)
                .map(|k| (args[k].cells(), criteria_matcher(&scalar(k + 1))))
                .collect();
            let n = pairs.first().map_or(0, |(cells, _)| cells.len());
            let pool = if upper == "COUNTIFS" {
                Vec::new()
            } else {
                args[0].cells()
            };
            let (mut sum, mut count) = (0.0, 0usize);
            for i in 0..n {
                if pairs.iter().all(|(cells, m)| cells.get(i).is_some_and(m)) {
                    count += 1;
                    if upper != "COUNTIFS" {
                        sum += pool.get(i).map(Value::as_number).unwrap_or(0.0);
                    }
                }
            }
            match upper {
                "COUNTIFS" => Value::Number(count as f64),
                "SUMIFS" => Value::Number(sum),
                _ if count == 0 => return Err(EvalErr::Div0),
                _ => Value::Number(sum / count as f64),
            }
        }

        // CHOOSE(index, value1, value2, …) — 1-based pick.
        // XLOOKUP(needle, lookup_range, return_range, [if_not_found]) — exact.
        "XLOOKUP" => {
            if args.len() < 3 {
                return Err(EvalErr::Value);
            }
            let needle = scalar(0);
            let look = args[1].cells();
            let ret = args[2].cells();
            match look
                .iter()
                .position(|v| compare_values(v, &needle) == Some(std::cmp::Ordering::Equal))
            {
                Some(i) => ret.get(i).cloned().ok_or(EvalErr::Ref)?,
                None if args.len() >= 4 => scalar(3),
                None => return Err(EvalErr::Na),
            }
        }

        // LOOKUP(needle, lookup_vector, [result_vector]) — last value ≤ needle
        // (assumes the lookup vector is sorted ascending, like Excel).
        "LOOKUP" => {
            if args.len() < 2 {
                return Err(EvalErr::Value);
            }
            let needle = scalar(0);
            let look = args[1].cells();
            let res = if args.len() >= 3 {
                args[2].cells()
            } else {
                look.clone()
            };
            let mut best: Option<usize> = None;
            for (i, v) in look.iter().enumerate() {
                if matches!(
                    compare_values(v, &needle),
                    Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
                ) {
                    best = Some(i);
                }
            }
            match best {
                Some(i) => res.get(i).cloned().unwrap_or(Value::Number(0.0)),
                None => return Err(EvalErr::Na),
            }
        }

        // TEXTJOIN(delimiter, ignore_empty, text…)
        "TEXTJOIN" => {
            if args.len() < 3 {
                return Err(EvalErr::Value);
            }
            let delim = text(0);
            let ignore_empty = scalar(1).is_truthy();
            let parts: Vec<String> = flatten_values(&args[2..])
                .iter()
                .map(Value::as_text)
                .filter(|s| !(ignore_empty && s.is_empty()))
                .collect();
            Value::Text(parts.join(&delim))
        }

        // SUBSTITUTE(text, old, new, [instance]) — replace by content.
        "SUBSTITUTE" => {
            if args.len() < 3 {
                return Err(EvalErr::Value);
            }
            let (s, old, new) = (text(0), text(1), text(2));
            if old.is_empty() {
                Value::Text(s)
            } else if args.len() >= 4 {
                Value::Text(substitute_nth(&s, &old, &new, num(3) as usize))
            } else {
                Value::Text(s.replace(&old, &new))
            }
        }

        // REPLACE(text, start, num_chars, new_text) — replace by position.
        "REPLACE" => {
            if args.len() < 4 {
                return Err(EvalErr::Value);
            }
            let chars: Vec<char> = text(0).chars().collect();
            let start = (num(1).max(1.0) as usize).saturating_sub(1);
            let count = num(2).max(0.0) as usize;
            let mut out: String = chars.iter().take(start).collect();
            out.push_str(&text(3));
            out.extend(chars.iter().skip(start + count));
            Value::Text(out)
        }

        // FIND (case-sensitive) / SEARCH (case-insensitive) → 1-based position;
        // #VALUE! when not found. Character-indexed (Unicode-safe).
        "FIND" | "SEARCH" => {
            if args.len() < 2 {
                return Err(EvalErr::Value);
            }
            let (needle, hay) = (text(0), text(1));
            let start = if args.len() >= 3 {
                (num(2).max(1.0) as usize).saturating_sub(1)
            } else {
                0
            };
            let (hay_c, needle_c): (Vec<char>, Vec<char>) = if upper == "SEARCH" {
                (
                    hay.to_lowercase().chars().collect(),
                    needle.to_lowercase().chars().collect(),
                )
            } else {
                (hay.chars().collect(), needle.chars().collect())
            };
            match find_subsequence(&hay_c, &needle_c, start) {
                Some(i) => Value::Number((i + 1) as f64),
                None => return Err(EvalErr::Value),
            }
        }

        // VALUE(text) — coerce numeric/date text to a number; else #VALUE!.
        "VALUE" => {
            let t = text(0);
            let t = t.trim();
            if let Ok(n) = t.parse::<f64>() {
                Value::Number(n)
            } else if let Some(serial) = crate::core::date::parse_date(t) {
                Value::Number(serial)
            } else {
                return Err(EvalErr::Value);
            }
        }

        // COUNTA counts non-blank cells; COUNTBLANK counts blanks (issue #36).
        "COUNTA" => Value::Number(
            flatten_values(args)
                .iter()
                .filter(|v| !matches!(v, Value::Blank))
                .count() as f64,
        ),
        "COUNTBLANK" => Value::Number(
            flatten_values(args)
                .iter()
                .filter(|v| matches!(v, Value::Blank))
                .count() as f64,
        ),

        // HYPERLINK(url, [label]): returns the label text (or the URL if no
        // label). The cell-level link is stored via set_cell_link; the formula
        // display shows the label making the cell effectively clickable when
        // the link property is set independently.
        "HYPERLINK" => {
            let url = text(0);
            let label = if args.len() > 1 { text(1) } else { url.clone() };
            Value::Text(label)
        }

        // WEBSERVICE(url) / IMPORTXML(url, xpath): these need network
        // access, which the wasm sandbox does not allow (no
        // synchronous fetch). They're registered so the parser
        // doesn't emit #NAME?; the runtime surfaces #VALUE! to
        // signal "not available in this build". A host that needs
        // these can implement them in JS via on_change + a fetch
        // and write the result back into the sheet.
        "WEBSERVICE" | "IMPORTXML" | "IMPORTHTML" | "IMPORTRANGE" | "IMPORTDATA" => {
            return Err(EvalErr::Value)
        }

        // IF / IFS / CHOOSE are short-circuit and handled upstream in
        // eval_expr's Token::Function arm (see eval_lazy, issue #38), so they
        // never reach this eager dispatch.
        _ => return Ok(None),
    };
    Ok(Some(v))
}

/// Replace only the `nth` (1-based) occurrence of `old` with `new`. `nth == 0`
/// or fewer than `nth` occurrences leaves the string unchanged. Used by
/// `SUBSTITUTE`'s optional instance argument.
fn substitute_nth(s: &str, old: &str, new: &str, nth: usize) -> String {
    if nth == 0 {
        return s.to_string();
    }
    let mut count = 0;
    let mut result = String::new();
    let mut rest = s;
    while let Some(pos) = rest.find(old) {
        count += 1;
        if count == nth {
            result.push_str(&rest[..pos]);
            result.push_str(new);
            result.push_str(&rest[pos + old.len()..]);
            return result;
        }
        result.push_str(&rest[..pos + old.len()]);
        rest = &rest[pos + old.len()..];
    }
    result.push_str(rest);
    result
}

/// First char index ≥ `start` at which `needle` occurs in `hay` (both already
/// case-folded by the caller for `SEARCH`). An empty needle matches at `start`.
fn find_subsequence(hay: &[char], needle: &[char], start: usize) -> Option<usize> {
    if needle.is_empty() {
        return (start <= hay.len()).then_some(start);
    }
    if needle.len() > hay.len() {
        return None;
    }
    (start..=hay.len() - needle.len()).find(|&i| hay[i..i + needle.len()] == *needle)
}

/// Row-major transpose of a rectangular grid.
fn transpose(grid: &[Vec<Value>]) -> Vec<Vec<Value>> {
    let cols = grid.first().map_or(0, Vec::len);
    (0..cols)
        .map(|c| grid.iter().map(|row| row[c].clone()).collect())
        .collect()
}

/// Element-wise row equality with Excel comparison semantics (numbers
/// numerically, text case-insensitively) for UNIQUE (issue #33).
fn rows_equal(a: &[Value], b: &[Value]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(x, y)| compare_values(x, y) == Some(std::cmp::Ordering::Equal))
}

// RANDARRAY's PRNG (issue #33): a tiny splitmix64 stream. `js_sys::Math::random`
// isn't callable from the native test build, and `SystemTime` panics on
// wasm32-unknown-unknown, so the stream is seeded with a fixed constant —
// successive calls differ, which is all a spreadsheet needs here.
thread_local! {
    static RAND_STATE: std::cell::Cell<u64> = const { std::cell::Cell::new(0x9E37_79B9_7F4A_7C15) };
}

/// Uniform in [0, 1).
fn next_rand() -> f64 {
    RAND_STATE.with(|s| {
        let mut z = s.get().wrapping_add(0x9E37_79B9_7F4A_7C15);
        s.set(z);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        ((z ^ (z >> 31)) >> 11) as f64 / (1u64 << 53) as f64
    })
}

/// Dynamic-array functions (issue #33): FILTER / SORT / SORTBY / UNIQUE /
/// SEQUENCE / RANDARRAY return a `Value::Array` that spills at a formula's
/// top level (see `recompute_spills`) and keeps its shape as a nested
/// argument. `Ok(None)` means "not an array function" — fall through.
fn apply_array_function(upper: &str, fargs: &[Arg]) -> Result<Option<Value>, EvalErr> {
    // A grid result never leaves here empty: an empty result is #CALC!,
    // matching Excel.
    let non_empty = |g: Vec<Vec<Value>>| -> Result<Option<Value>, EvalErr> {
        if g.is_empty() || g.iter().all(Vec::is_empty) {
            Err(EvalErr::Calc)
        } else {
            Ok(Some(Value::Array(g)))
        }
    };
    match upper {
        "FILTER" => {
            let [array, include, ..] = fargs else {
                return Err(EvalErr::Value);
            };
            let grid = array.grid();
            let inc = include.grid();
            let (rows, cols) = (grid.len(), grid.first().map_or(0, Vec::len));
            let (irows, icols) = (inc.len(), inc.first().map_or(0, Vec::len));
            let kept: Vec<Vec<Value>> = if irows == rows && icols == 1 {
                // Column-shaped include: keep matching rows.
                grid.into_iter()
                    .zip(&inc)
                    .filter(|(_, i)| i[0].is_truthy())
                    .map(|(r, _)| r)
                    .collect()
            } else if irows == 1 && icols == cols {
                // Row-shaped include: keep matching columns.
                let keep: Vec<bool> = inc[0].iter().map(Value::is_truthy).collect();
                grid.into_iter()
                    .map(|row| {
                        row.into_iter()
                            .zip(&keep)
                            .filter(|(_, k)| **k)
                            .map(|(v, _)| v)
                            .collect()
                    })
                    .collect()
            } else {
                return Err(EvalErr::Value);
            };
            if kept.is_empty() || kept.iter().all(Vec::is_empty) {
                // No matches: the if_empty argument, else #CALC!.
                return match fargs.get(2) {
                    Some(v) => Ok(Some(v.to_scalar())),
                    None => Err(EvalErr::Calc),
                };
            }
            Ok(Some(Value::Array(kept)))
        }
        "SORT" => {
            let array = fargs.first().ok_or(EvalErr::Value)?;
            let by_col = fargs
                .get(3)
                .map(|a| a.to_scalar().is_truthy())
                .unwrap_or(false);
            let mut grid = array.grid();
            if by_col {
                grid = transpose(&grid);
            }
            let idx = match fargs.get(1) {
                Some(a) => {
                    let i = a.to_scalar().as_number();
                    // Reject non-finite (NaN / ±inf) up front: a `NaN as
                    // usize` saturates to 0, which would slip past
                    // `i < 1.0` and trigger `0_usize - 1` integer
                    // underflow on the next line (issue #55). `is_finite`
                    // catches NaN, +inf, and -inf uniformly.
                    if !i.is_finite() || i < 1.0 || (i as usize) > grid.first().map_or(0, Vec::len)
                    {
                        return Err(EvalErr::Value);
                    }
                    i as usize - 1
                }
                None => 0,
            };
            let desc = fargs
                .get(2)
                .map(|a| a.to_scalar().as_number() < 0.0)
                .unwrap_or(false);
            grid.sort_by(|a, b| {
                let ord = compare_values(&a[idx], &b[idx]).unwrap_or(std::cmp::Ordering::Equal);
                if desc {
                    ord.reverse()
                } else {
                    ord
                }
            });
            if by_col {
                grid = transpose(&grid);
            }
            non_empty(grid)
        }
        "SORTBY" => {
            let array = fargs.first().ok_or(EvalErr::Value)?;
            let grid = array.grid();
            let n = grid.len();
            // (by_array, ascending) pairs; each sort_order is optional.
            let mut keys: Vec<(Vec<Value>, bool)> = Vec::new();
            let mut i = 1;
            while i < fargs.len() {
                let by = fargs[i].cells();
                if by.len() != n {
                    return Err(EvalErr::Value);
                }
                let asc = match fargs.get(i + 1) {
                    // A length-1 argument here is a sort_order scalar, not
                    // the next by_array (which must have n elements).
                    Some(a) if a.cells().len() == 1 && n != 1 => {
                        i += 2;
                        a.to_scalar().as_number() >= 0.0
                    }
                    _ => {
                        i += 1;
                        true
                    }
                };
                keys.push((by, asc));
            }
            if keys.is_empty() {
                return Err(EvalErr::Value);
            }
            let mut order: Vec<usize> = (0..n).collect();
            order.sort_by(|&a, &b| {
                for (by, asc) in &keys {
                    let ord = compare_values(&by[a], &by[b]).unwrap_or(std::cmp::Ordering::Equal);
                    if ord != std::cmp::Ordering::Equal {
                        return if *asc { ord } else { ord.reverse() };
                    }
                }
                std::cmp::Ordering::Equal
            });
            non_empty(order.into_iter().map(|r| grid[r].clone()).collect())
        }
        "UNIQUE" => {
            let array = fargs.first().ok_or(EvalErr::Value)?;
            let by_col = fargs
                .get(1)
                .map(|a| a.to_scalar().is_truthy())
                .unwrap_or(false);
            let exactly_once = fargs
                .get(2)
                .map(|a| a.to_scalar().is_truthy())
                .unwrap_or(false);
            let mut grid = array.grid();
            if by_col {
                grid = transpose(&grid);
            }
            let mut out: Vec<Vec<Value>> = Vec::new();
            if exactly_once {
                for row in &grid {
                    if grid.iter().filter(|r| rows_equal(r, row)).count() == 1 {
                        out.push(row.clone());
                    }
                }
            } else {
                for row in &grid {
                    if !out.iter().any(|r| rows_equal(r, row)) {
                        out.push(row.clone());
                    }
                }
            }
            if by_col {
                out = transpose(&out);
            }
            non_empty(out)
        }
        "SEQUENCE" => {
            let num = |i: usize, default: f64| {
                fargs
                    .get(i)
                    .map(|a| a.to_scalar().as_number())
                    .unwrap_or(default)
            };
            let rows = num(0, 1.0);
            let cols = num(1, 1.0);
            if rows < 0.0 || cols < 0.0 {
                return Err(EvalErr::Value);
            }
            let (rows, cols) = (rows as usize, cols as usize);
            if rows == 0 || cols == 0 {
                return Err(EvalErr::Calc);
            }
            if rows.saturating_mul(cols) > 1_000_000 {
                return Err(EvalErr::Num);
            }
            let (start, step) = (num(2, 1.0), num(3, 1.0));
            non_empty(
                (0..rows)
                    .map(|r| {
                        (0..cols)
                            .map(|c| Value::Number(start + (r * cols + c) as f64 * step))
                            .collect()
                    })
                    .collect(),
            )
        }
        "RANDARRAY" => {
            let num = |i: usize, default: f64| {
                fargs
                    .get(i)
                    .map(|a| a.to_scalar().as_number())
                    .unwrap_or(default)
            };
            let rows = num(0, 1.0);
            let cols = num(1, 1.0);
            if rows < 1.0 || cols < 1.0 {
                return Err(EvalErr::Value);
            }
            let (rows, cols) = (rows as usize, cols as usize);
            if rows.saturating_mul(cols) > 1_000_000 {
                return Err(EvalErr::Num);
            }
            let (min, max) = (num(2, 0.0), num(3, 1.0));
            if min > max {
                return Err(EvalErr::Value);
            }
            let whole = fargs
                .get(4)
                .map(|a| a.to_scalar().is_truthy())
                .unwrap_or(false);
            non_empty(
                (0..rows)
                    .map(|_| {
                        (0..cols)
                            .map(|_| {
                                let v = if whole {
                                    min.floor()
                                        + (next_rand() * (max.floor() - min.floor() + 1.0)).floor()
                                } else {
                                    min + next_rand() * (max - min)
                                };
                                Value::Number(v)
                            })
                            .collect()
                    })
                    .collect(),
            )
        }
        _ => Ok(None),
    }
}

fn apply_function(name: &str, fargs: &[Arg]) -> Result<Value, EvalErr> {
    let upper = name.to_uppercase();
    // Dynamic-array functions return grids that spill (issue #33).
    if let Some(v) = apply_array_function(&upper, fargs)? {
        return Ok(v);
    }
    // Text / criteria / lookup functions need values or range shape (issue #2).
    if let Some(v) = apply_special_function(&upper, fargs)? {
        return Ok(v);
    }
    // Everything else works on the flattened numeric view, exactly as this
    // engine always has (numeric text coerces; other text counts as 0).
    let args: Vec<f64> = flatten_values(fargs).iter().map(Value::as_number).collect();
    let args: &[f64] = &args;
    let first = args.first().copied().unwrap_or(0.0);
    let second = args.get(1).copied().unwrap_or(0.0);
    let v = match upper.as_str() {
        // Aggregation
        "SUM" => args.iter().sum(),
        "PRODUCT" => args.iter().product(),
        "AVERAGE" | "AVG" => {
            if args.is_empty() {
                0.0
            } else {
                args.iter().sum::<f64>() / args.len() as f64
            }
        }
        "MAX" => finite_or(args.iter().cloned().fold(f64::NEG_INFINITY, f64::max)),
        "MIN" => finite_or(args.iter().cloned().fold(f64::INFINITY, f64::min)),
        "COUNT" => args.len() as f64,
        "SUMSQ" => args.iter().map(|v| v * v).sum(),
        "MEDIAN" => median(args),
        "VAR" => variance(args),
        "STDEV" => variance(args).sqrt(),
        "STDEV.S" => variance(args).sqrt(),
        "STDEV.P" => population_variance(args).sqrt(),
        "VAR.P" => population_variance(args),
        "VAR.S" => variance(args),
        "PERCENTILE.INC" => percentile_inc(args, second),
        "QUARTILE.INC" => quartile_inc(args, second),
        "RANK.EQ" => rank_eq(args, first),
        "COVARIANCE.P" => covariance_p(args),
        "CORREL" => correlation(args),

        // Logical (non-zero is truthy; returns 1.0/0.0)
        "AND" => bool_f64(!args.is_empty() && args.iter().all(|&v| v != 0.0)),
        "OR" => bool_f64(args.iter().any(|&v| v != 0.0)),
        "NOT" => bool_f64(first == 0.0),
        "TRUE" => 1.0,
        "FALSE" => 0.0,
        // IF / IFS / CHOOSE are short-circuit and handled upstream in eval_lazy
        // (issue #38) — they never reach this numeric dispatch.

        // Math
        "ABS" => first.abs(),
        "SIGN" => {
            if first > 0.0 {
                1.0
            } else if first < 0.0 {
                -1.0
            } else {
                0.0
            }
        }
        "MOD" => {
            if second == 0.0 {
                return Err(EvalErr::Div0);
            }
            first - second * (first / second).floor()
        }
        "POWER" => first.powf(second),
        "SQRT" => first.sqrt(),
        "EXP" => first.exp(),
        "LN" => first.ln(),
        "LOG10" => first.log10(),
        "LOG" => {
            if args.len() >= 2 {
                first.log(second)
            } else {
                first.log10()
            }
        }
        "INT" => first.floor(),
        "ROUND" => round_to(first, second),
        "ROUNDUP" => round_dir(first, second, true),
        "ROUNDDOWN" => round_dir(first, second, false),
        "CEILING" => {
            let sig = if args.len() >= 2 { second } else { 1.0 };
            if sig == 0.0 {
                0.0
            } else {
                (first / sig).ceil() * sig
            }
        }
        "FLOOR" => {
            let sig = if args.len() >= 2 { second } else { 1.0 };
            if sig == 0.0 {
                0.0
            } else {
                (first / sig).floor() * sig
            }
        }

        // Date & time (serial numbers; see core::date)
        "DATE" => crate::core::date::to_serial(
            first as i64,
            second as i64,
            args.get(2).copied().unwrap_or(1.0) as i64,
        ),
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

        // --- Financial functions ---
        "PMT" => pmt(
            first,
            second,
            args.get(2).copied().unwrap_or(0.0),
            args.get(3).copied().unwrap_or(0.0),
            args.get(4).copied().unwrap_or(0.0),
        ),
        "PV" => pv(
            first,
            second,
            args.get(2).copied().unwrap_or(0.0),
            args.get(3).copied().unwrap_or(0.0),
            args.get(4).copied().unwrap_or(0.0),
        ),
        "FV" => fv(
            first,
            second,
            args.get(2).copied().unwrap_or(0.0),
            args.get(3).copied().unwrap_or(0.0),
            args.get(4).copied().unwrap_or(0.0),
        ),
        "NPV" => npv(first, &args[1..]),
        "IRR" => {
            let guess = if args.len() > 1 { second } else { 0.1 };
            irr(args, guess)
        }
        "RATE" => rate(
            second,
            args.get(2).copied().unwrap_or(0.0),
            first,
            args.get(3).copied().unwrap_or(0.0),
            args.get(4).copied().unwrap_or(0.0),
            args.get(5).copied().unwrap_or(0.1),
        ),
        "SLN" => sln(first, second, args.get(2).copied().unwrap_or(0.0)),
        "DB" => db(
            first,
            second,
            args.get(2).copied().unwrap_or(0.0),
            args.get(3).copied().unwrap_or(1.0),
            args.get(4).copied().unwrap_or(12.0),
        ),
        "DDB" => ddb(
            first,
            second,
            args.get(2).copied().unwrap_or(0.0),
            args.get(3).copied().unwrap_or(1.0),
            args.get(4).copied().unwrap_or(2.0),
        ),
        "PPMT" => ppmt(
            first,
            second,
            args.get(2).copied().unwrap_or(0.0),
            args.get(3).copied().unwrap_or(0.0),
            args.get(4).copied().unwrap_or(0.0),
            args.get(5).copied().unwrap_or(0.0),
        ),
        "IPMT" => ipmt(
            first,
            second,
            args.get(2).copied().unwrap_or(0.0),
            args.get(3).copied().unwrap_or(0.0),
            args.get(4).copied().unwrap_or(0.0),
            args.get(5).copied().unwrap_or(0.0),
        ),
        "XNPV" => {
            if args.len() < 3 || !(args.len() - 1).is_multiple_of(2) {
                return Err(EvalErr::Value);
            }
            xnpv(first, &args[1..])
        }
        "XIRR" => {
            if args.len() < 3 || !(args.len() - 1).is_multiple_of(2) {
                return Err(EvalErr::Value);
            }
            let guess = if args.len() > 1 { second } else { 0.1 };
            xirr(&args[1..], guess)
        }

        // Unknown function name.
        _ => return Err(EvalErr::Name),
    };

    // A finite-domain math function that produced NaN/∞ is a #NUM! error
    // (e.g. SQRT(-1), LN(0)).
    if matches!(
        upper.as_str(),
        "SQRT" | "LN" | "LOG" | "LOG10" | "POWER" | "RATE" | "IRR" | "XIRR"
    ) && !v.is_finite()
    {
        return Err(EvalErr::Num);
    }
    Ok(Value::Number(v))
}

fn bool_f64(b: bool) -> f64 {
    if b {
        1.0
    } else {
        0.0
    }
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
    if v.is_finite() {
        v
    } else {
        0.0
    }
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
    let secs =
        d.get_hours() as f64 * 3600.0 + d.get_minutes() as f64 * 60.0 + d.get_seconds() as f64;
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

// ---------------------------------------------------------------------------
// Statistical helpers
// ---------------------------------------------------------------------------

/// Population variance (n denominator), matching Excel's VAR.P.
fn population_variance(args: &[f64]) -> f64 {
    let n = args.len();
    if n < 1 {
        return 0.0;
    }
    let mean = args.iter().sum::<f64>() / n as f64;
    args.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n as f64
}

/// PERCENTILE.INC(array, k) — k in [0, 1]. Linear interpolation.
fn percentile_inc(args: &[f64], k: f64) -> f64 {
    if args.is_empty() {
        return 0.0;
    }
    let k = k.clamp(0.0, 1.0);
    let mut v: Vec<f64> = args.to_vec();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = v.len();
    if n == 1 {
        return v[0];
    }
    let pos = k * (n - 1) as f64;
    let lo = pos.floor() as usize;
    let hi = pos.ceil() as usize;
    if lo == hi {
        v[lo]
    } else {
        let frac = pos - lo as f64;
        v[lo] + (v[hi] - v[lo]) * frac
    }
}

/// QUARTILE.INC(array, quart) — quart: 0=min, 1=25%, 2=median, 3=75%, 4=max.
fn quartile_inc(args: &[f64], quart: f64) -> f64 {
    let q = quart.clamp(0.0, 4.0);
    if q == 0.0 {
        return args.iter().cloned().fold(f64::INFINITY, f64::min);
    }
    if q == 4.0 {
        return args.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    }
    percentile_inc(args, q * 0.25)
}

/// RANK.EQ(number, ref) — rank of `number` in descending order (Excel default).
/// `number` is `first` arg, `ref` is the rest of flattened args.
fn rank_eq(args: &[f64], value: f64) -> f64 {
    if args.len() < 2 {
        return 0.0;
    }
    let data = &args[1..]; // skip the value itself
    let rank = 1 + data.iter().filter(|&&x| x > value).count();
    rank as f64
}

/// Population covariance.
fn covariance_p(args: &[f64]) -> f64 {
    let n = args.len();
    if n < 2 || !n.is_multiple_of(2) {
        return 0.0;
    }
    let half = n / 2;
    let (xs, ys) = (&args[..half], &args[half..]);
    let mx = xs.iter().sum::<f64>() / half as f64;
    let my = ys.iter().sum::<f64>() / half as f64;
    xs.iter()
        .zip(ys.iter())
        .map(|(x, y)| (x - mx) * (y - my))
        .sum::<f64>()
        / half as f64
}

/// Pearson correlation coefficient.
fn correlation(args: &[f64]) -> f64 {
    let n = args.len();
    if n < 2 || !n.is_multiple_of(2) {
        return 0.0;
    }
    let half = n / 2;
    let (xs, ys) = (&args[..half], &args[half..]);
    let mx = xs.iter().sum::<f64>() / half as f64;
    let my = ys.iter().sum::<f64>() / half as f64;
    let cov = xs
        .iter()
        .zip(ys.iter())
        .map(|(x, y)| (x - mx) * (y - my))
        .sum::<f64>()
        / half as f64;
    let sx = (xs.iter().map(|x| (x - mx).powi(2)).sum::<f64>() / half as f64).sqrt();
    let sy = (ys.iter().map(|y| (y - my).powi(2)).sum::<f64>() / half as f64).sqrt();
    if sx == 0.0 || sy == 0.0 {
        0.0
    } else {
        cov / (sx * sy)
    }
}

// ---------------------------------------------------------------------------
// Financial function implementations
// ---------------------------------------------------------------------------

/// PMT(rate, nper, pv, [fv], [type_])
/// type_=0: payments at end of period (default); 1: beginning.
/// Signed so a positive PV (loan) gives a negative PMT (payment).
fn pmt(rate: f64, nper: f64, pv: f64, fv: f64, type_: f64) -> f64 {
    if rate == 0.0 {
        return -(pv + fv) / nper;
    }
    let f = (1.0 + rate).powf(nper);
    let pmt_val = -(pv * f + fv) * rate / (f - 1.0);
    if type_ == 1.0 {
        pmt_val / (1.0 + rate)
    } else {
        pmt_val
    }
}

/// PV(rate, nper, pmt, [fv], [type_])
fn pv(rate: f64, nper: f64, pmt: f64, fv: f64, type_: f64) -> f64 {
    if rate == 0.0 {
        return -fv - pmt * nper;
    }
    let f = (1.0 + rate).powf(nper);
    let start = if type_ == 1.0 { 1.0 + rate } else { 1.0 };
    let pv_factor = (1.0 - 1.0 / f) / rate;
    -fv / f - pmt * start * pv_factor
}

/// FV(rate, nper, pmt, [pv], [type_])
fn fv(rate: f64, nper: f64, pmt: f64, pv: f64, type_: f64) -> f64 {
    if rate == 0.0 {
        return -pv - pmt * nper;
    }
    let f = (1.0 + rate).powf(nper);
    let start = if type_ == 1.0 { 1.0 + rate } else { 1.0 };
    -pv * f - pmt * start * (f - 1.0) / rate
}

/// NPV(rate, values...)
fn npv(rate: f64, values: &[f64]) -> f64 {
    values
        .iter()
        .enumerate()
        .map(|(i, &v)| v / (1.0 + rate).powf(i as f64 + 1.0))
        .sum()
}

/// IRR(values, [guess]) — internal rate of return via Newton's method.
fn irr(cashflows: &[f64], guess: f64) -> f64 {
    if cashflows.len() < 2 {
        return 0.0;
    }
    let mut r = guess;
    for _ in 0..100 {
        let (npv, dnpv) = cashflows
            .iter()
            .enumerate()
            .fold((0.0, 0.0), |(n, d), (i, &cf)| {
                let denom = (1.0 + r).powf(i as f64);
                (n + cf / denom, d - i as f64 * cf / (denom * (1.0 + r)))
            });
        if dnpv.abs() < 1e-15 {
            break;
        }
        let dr = npv / dnpv;
        r -= dr;
        if dr.abs() < 1e-8 {
            break;
        }
    }
    r
}

/// RATE(nper, pmt, pv, [fv], [type_], [guess]) via Newton iteration.
fn rate(nper: f64, pmt: f64, pv: f64, fv: f64, type_: f64, guess: f64) -> f64 {
    let mut r = guess;
    for _ in 0..100 {
        let f = (1.0 + r).powf(nper);
        let df = nper * (1.0 + r).powf(nper - 1.0);
        let start = if type_ == 1.0 { 1.0 + r } else { 1.0 };
        let dstart = if type_ == 1.0 { 1.0 } else { 0.0 };
        let pv_t = pv * f + pmt * start * (f - 1.0) / r + fv;
        let dpv = pv * df + pmt * (dstart * (f - 1.0) / r + start * (df / r - (f - 1.0) / (r * r)));
        if dpv.abs() < 1e-15 {
            break;
        }
        let dr = pv_t / dpv;
        r -= dr;
        if dr.abs() < 1e-8 {
            break;
        }
    }
    r
}

/// SLN(cost, salvage, life) — straight-line depreciation.
fn sln(cost: f64, salvage: f64, life: f64) -> f64 {
    if life == 0.0 {
        return 0.0;
    }
    (cost - salvage) / life
}

/// DB(cost, salvage, life, period, [month]) — fixed-declining balance.
fn db(cost: f64, salvage: f64, life: f64, period: f64, month: f64) -> f64 {
    if life <= 0.0 || period < 1.0 {
        return 0.0;
    }
    let rate = 1.0 - (salvage / cost).powf(1.0 / life);
    let rate = (rate * 1000.0).round() / 1000.0; // round to 3 decimal places
    let first_period_rate = rate * month / 12.0;
    let mut total = cost - cost * first_period_rate;
    if period == 1.0 {
        return cost * first_period_rate;
    }
    for _ in 2..(period as usize).min(life as usize) {
        total -= total * rate;
    }
    if period >= life {
        // Final period: remaining book value minus salvage
        (total - salvage).max(0.0)
    } else {
        total * rate
    }
}

/// DDB(cost, salvage, life, period, [factor]) — double-declining balance.
fn ddb(cost: f64, salvage: f64, life: f64, period: f64, factor: f64) -> f64 {
    if life <= 0.0 || period < 1.0 || period > life {
        return 0.0;
    }
    let rate = factor / life;
    // DDB returns the depreciation for the *requested* period, not
    // the cumulative sum, so we just walk the book value forward
    // to that period and return the per-period delta. Earlier
    // periods' deltas are discarded.
    let mut book = cost;
    for _ in 1..(period as usize) {
        let dep = (book * rate).min(book - salvage).max(0.0);
        book -= dep;
    }
    (book * rate).min(book - salvage).max(0.0)
}

/// PPMT(rate, per, nper, pv, [fv], [type_])
fn ppmt(rate: f64, per: f64, nper: f64, pv_val: f64, fv: f64, type_: f64) -> f64 {
    if per < 1.0 || per > nper {
        return 0.0;
    }
    let payment = pmt(rate, nper, pv_val, fv, type_);
    let interest = ipmt(rate, per, nper, pv_val, fv, type_);
    payment - interest
}

/// IPMT(rate, per, nper, pv, [fv], [type_])
fn ipmt(rate: f64, per: f64, nper: f64, pv_val: f64, fv: f64, type_: f64) -> f64 {
    if per < 1.0 || per > nper {
        return 0.0;
    }
    if rate == 0.0 {
        return 0.0;
    }
    let payment = pmt(rate, nper, pv_val, fv, type_);
    let start_balance = if per == 1.0 {
        pv_val
    } else {
        let prior_pv = if type_ == 1.0 {
            pv(rate, per - 1.0, payment, fv, 1.0)
        } else {
            pv(rate, per - 1.0, payment, fv, 0.0)
        };
        -prior_pv
    };
    start_balance * rate
}

/// XNPV(rate, values_and_dates...) where args are [value0, date0, value1, date1, ...]
fn xnpv(rate: f64, values_and_dates: &[f64]) -> f64 {
    let n = values_and_dates.len();
    let d0 = values_and_dates[1]; // first date as reference
    let mut total = 0.0;
    for i in (0..n).step_by(2) {
        let v = values_and_dates[i];
        let d = values_and_dates[i + 1];
        let days = d - d0;
        total += v / (1.0 + rate).powf(days / 365.0);
    }
    total
}

/// XIRR(values_and_dates..., [guess]) — IRR for irregular cash flows.
fn xirr(values_and_dates: &[f64], guess: f64) -> f64 {
    let n = values_and_dates.len();
    if n < 4 {
        return 0.0;
    }
    let d0 = values_and_dates[1];
    let mut r = guess;
    for _ in 0..100 {
        let (npv, dnpv) = (0..n).step_by(2).fold((0.0, 0.0), |(n_sum, d_sum), i| {
            let v = values_and_dates[i];
            let d = values_and_dates[i + 1];
            let t = (d - d0) / 365.0;
            let denom = (1.0 + r).powf(t);
            (n_sum + v / denom, d_sum - t * v / (denom * (1.0 + r)))
        });
        if dnpv.abs() < 1e-15 {
            break;
        }
        let dr = npv / dnpv;
        r -= dr;
        if dr.abs() < 1e-8 {
            break;
        }
    }
    r
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

    // Bare TRUE/FALSE are recognized boolean literals (issue #39). This engine
    // models booleans as 1/0, so they render and flow through as 1/0.
    #[test]
    fn boolean_literals() {
        assert_eq!(eval("=TRUE", &[]), "1");
        assert_eq!(eval("=FALSE", &[]), "0");
        assert_eq!(eval("=true", &[]), "1"); // case-insensitive
        assert_eq!(eval("=False", &[]), "0");
        // Flow through functions.
        assert_eq!(eval("=IF(TRUE, 7, 9)", &[]), "7");
        assert_eq!(eval("=IF(FALSE, 7, 9)", &[]), "9");
        assert_eq!(eval("=AND(TRUE, FALSE)", &[]), "0");
        assert_eq!(eval("=OR(TRUE, FALSE)", &[]), "1");
        // The TRUE()/FALSE() functions still work — the literal must not shadow
        // the `Name(` -> Function tokenization.
        assert_eq!(eval("=TRUE()", &[]), "1");
        assert_eq!(eval("=FALSE()", &[]), "0");
        // VLOOKUP/HLOOKUP accept the FALSE literal as the exact-match flag.
        assert_eq!(
            eval_with(
                &[(0, 0, "Bob"), (0, 1, "200")],
                "=VLOOKUP(\"Bob\", A1:B2, 2, FALSE)"
            ),
            "200"
        );
        assert_eq!(
            eval_with(
                &[(0, 0, "Bob"), (1, 0, "200")],
                "=HLOOKUP(\"Bob\", A1:A2, 2, FALSE)"
            ),
            "200"
        );
    }

    // IF/IFS/CHOOSE are short-circuit: a not-taken branch is never evaluated,
    // so an error in it does not propagate (issue #38).
    #[test]
    fn lazy_if_ifs_choose() {
        assert_eq!(eval("=IF(TRUE(), 1, 1/0)", &[]), "1");
        assert_eq!(eval("=IF(FALSE(), 1/0, 2)", &[]), "2");
        // Taken branches still work, text survives, condition errors propagate.
        assert_eq!(eval("=IF(1>0, 7, 9)", &[]), "7");
        assert_eq!(
            eval_with(&[(0, 0, "x")], "=IF(A1=\"x\", \"yes\", \"no\")"),
            "yes"
        );
        assert_eq!(eval("=IF(1/0, 1, 2)", &[]), "#DIV/0!"); // condition error propagates
                                                            // IFS: only the matched pair's value is evaluated.
        assert_eq!(eval("=IFS(FALSE(), 1/0, TRUE(), 5)", &[]), "5");
        assert_eq!(eval("=IFS(TRUE(), 5, FALSE(), 1/0)", &[]), "5");
        // CHOOSE: only the selected value is evaluated; 1-based index.
        assert_eq!(eval("=CHOOSE(2, 1/0, 42, 1/0)", &[]), "42");
        assert_eq!(eval("=CHOOSE(1, 10, 20)", &[]), "10");
        assert_eq!(eval("=CHOOSE(9, 1, 2)", &[]), "#VALUE!"); // out of range
    }

    // Position / reference functions (issue #37). `eval` evaluates at (50,50),
    // so the calling cell is row 51 / column 51 (1-based).
    #[test]
    fn position_functions() {
        // ROW()/COLUMN() use the calling cell when the argument is omitted.
        assert_eq!(eval("=ROW()", &[]), "51");
        assert_eq!(eval("=COLUMN()", &[]), "51");
        // ROW(ref)/COLUMN(ref) read the reference's coordinates, not its value.
        assert_eq!(eval("=ROW(C5)", &[]), "5");
        assert_eq!(eval("=COLUMN(C5)", &[]), "3");
        // ADDRESS builds an A1 string; abs_num 1=$A$1, 2=A$1, 3=$A1, 4=A1.
        assert_eq!(eval("=ADDRESS(2, 3)", &[]), "$C$2");
        assert_eq!(eval("=ADDRESS(2, 3, 4)", &[]), "C2");
        assert_eq!(eval("=ADDRESS(1, 1, 2)", &[]), "A$1");
        assert_eq!(eval("=ADDRESS(1, 1, 3)", &[]), "$A1");
        // INDIRECT resolves a reference built from a string.
        assert_eq!(eval_with(&[(0, 0, "42")], "=INDIRECT(\"A1\")"), "42");
        assert_eq!(eval("=INDIRECT(\"not a ref\")", &[]), "#REF!");
        // OFFSET shifts a reference; scalar context yields the top-left value.
        assert_eq!(eval_with(&[(2, 2, "hi")], "=OFFSET(A1, 2, 2)"), "hi"); // -> C3
                                                                           // OFFSET / INDIRECT ranges compose inside SUM.
        assert_eq!(
            eval_with(
                &[(0, 0, "1"), (1, 0, "2"), (2, 0, "3")],
                "=SUM(OFFSET(A1, 0, 0, 3, 1))"
            ),
            "6"
        );
        assert_eq!(
            eval_with(&[(0, 0, "10"), (1, 0, "20")], "=SUM(INDIRECT(\"A1:A2\"))"),
            "30"
        );
    }

    #[test]
    fn freeze_defaults_to_inactive() {
        let d = DataProxy::new("t");
        assert_eq!(d.freeze, (0, 0));
        assert!(!d.freeze_is_active());
    }

    #[test]
    fn set_freeze_activates_and_unfreeze_clears() {
        let mut d = DataProxy::new("t");
        d.set_freeze(2, 3);
        assert_eq!(d.freeze, (2, 3));
        assert!(d.freeze_is_active());
        d.set_freeze(0, 0); // the renderer's `unfreeze` path
        assert_eq!(d.freeze, (0, 0));
        assert!(!d.freeze_is_active());
    }

    // The freeze origins the toolbar menu produces (#18) must survive a
    // serialization roundtrip so frozen panes persist across save/load and
    // sheet switches.
    #[test]
    fn freeze_origins_survive_serialization_roundtrip() {
        // (label, origin) — top row, first column, panes-at-selection, unfrozen.
        for (label, origin) in [
            ("top-row", (1usize, 0usize)),
            ("first-col", (0, 1)),
            ("panes", (3, 2)),
            ("none", (0, 0)),
        ] {
            let mut src = DataProxy::new("t");
            src.set_freeze(origin.0, origin.1);

            let mut dst = DataProxy::new("t");
            dst.set_data(src.get_data());

            assert_eq!(dst.freeze, origin, "{label}: freeze origin not preserved");
            assert_eq!(
                dst.freeze_is_active(),
                origin != (0, 0),
                "{label}: freeze_is_active mismatch after roundtrip"
            );
        }
    }

    // --- Style interning must respect the border field ---
    //
    // `add_style` deduplicates identical styles via `styles_equal`. That
    // comparison originally omitted `border`, so a bordered style collapsed
    // into an existing borderless one, `set_cell_style` pointed the cell back
    // at the borderless entry, and the borders dropdown had no visible effect.

    #[test]
    fn add_style_distinguishes_border() {
        let mut d = DataProxy::new("t");
        let plain_idx = d.add_style(Style::default());
        let bordered = Style {
            border: Some(Border {
                top: Some(("thin".to_string(), "#000000".to_string())),
                ..Border::default()
            }),
            ..Style::default()
        };
        let bordered_idx = d.add_style(bordered);
        assert_ne!(
            plain_idx, bordered_idx,
            "a bordered style must not be interned as equal to a borderless one"
        );
        assert!(d.styles[bordered_idx].border.is_some());
    }

    #[test]
    fn border_survives_style_interning() {
        // End-to-end of the set_borders mechanism: seed a borderless style (as
        // any prior formatting or the default baseline would), then apply a
        // border to that cell and confirm get_cell_style still reports it.
        let mut d = DataProxy::new("t");
        let idx0 = d.add_style(Style::default());
        d.set_cell_style(0, 0, idx0);

        let mut style = d.get_cell_style(0, 0);
        let mut b = style.border.clone().unwrap_or_default();
        b.top = Some(("thin".to_string(), "#000000".to_string()));
        style.border = Some(b);
        let idx = d.add_style(style);
        d.set_cell_style(0, 0, idx);

        assert!(
            d.get_cell_style(0, 0).border.and_then(|b| b.top).is_some(),
            "top border must persist after style interning"
        );
    }

    #[test]
    fn border_diagonals_round_trip_through_serde() {
        // Set a border with both diagonals + a top edge, serialize
        // it, deserialize it back, and confirm everything came
        // through. This pins the wire format for the new fields.
        let original = Border {
            top: Some(("medium".to_string(), "#ff0000".to_string())),
            diagonal_up: Some(("double".to_string(), "#0000ff".to_string())),
            diagonal_down: Some(("dashed".to_string(), "#00ff00".to_string())),
            ..Border::default()
        };
        let json = serde_json::to_string(&original).unwrap();
        // Both diagonals must be in the JSON even though the four
        // edges default to None.
        assert!(json.contains("diagonal_up"), "json was: {json}");
        assert!(json.contains("diagonal_down"), "json was: {json}");
        let back: Border = serde_json::from_str(&json).unwrap();
        assert_eq!(back, original);
    }

    #[test]
    fn border_pre_1_x_workbook_loads_with_no_diagonals() {
        // A JSON blob with the pre-1.4 schema (no diagonal fields) must
        // still load — the `#[serde(default)]` on the new fields
        // fills them with None. This is the backward-compat pin.
        let legacy = r##"{
            "left": null,
            "right": null,
            "top":   ["thin", "#000000"],
            "bottom": null
        }"##;
        let b: Border = serde_json::from_str(legacy).unwrap();
        assert!(b.diagonal_up.is_none());
        assert!(b.diagonal_down.is_none());
        assert!(b.top.is_some());
    }

    #[test]
    fn border_default_serializes_to_no_diagonals() {
        // A border with only a single edge must NOT emit the
        // diagonal fields at all (skip_serializing_if = "Option::is_none").
        // Pre-1.4 readers and diff-based test fixtures rely on this.
        let b = Border {
            top: Some(("thin".to_string(), "#000000".to_string())),
            ..Border::default()
        };
        let json = serde_json::to_string(&b).unwrap();
        assert!(!json.contains("diagonal_up"));
        assert!(!json.contains("diagonal_down"));
    }

    // --- Hide / unhide + cell shift (issue #14) ---

    #[test]
    fn hide_unhide_rows_and_cols() {
        let mut d = DataProxy::new("t");
        assert!(!d.is_row_hidden(2));
        d.set_row_hidden(2, true);
        assert!(d.is_row_hidden(2));
        assert_eq!(d.get_row_height(2), 0.0); // collapsed for the renderer
        d.set_row_hidden(2, false);
        assert!(!d.is_row_hidden(2));
        assert!(d.get_row_height(2) > 0.0);

        assert!(!d.is_col_hidden(3));
        d.set_col_hidden(3, true);
        assert!(d.is_col_hidden(3));
        assert_eq!(d.get_col_width(3), 0.0);
        d.set_col_hidden(3, false);
        assert!(!d.is_col_hidden(3));
    }

    #[test]
    fn insert_cells_shift_down() {
        let mut d = DataProxy::new("t");
        d.set_cell_text(0, 0, "a0");
        d.set_cell_text(1, 0, "a1");
        d.set_cell_text(0, 1, "b0"); // untouched column
        d.insert_cells(0, 0, 0, 0, false);
        assert_eq!(d.get_cell_text(0, 0), ""); // vacated
        assert_eq!(d.get_cell_text(1, 0), "a0"); // pushed down
        assert_eq!(d.get_cell_text(2, 0), "a1");
        assert_eq!(d.get_cell_text(0, 1), "b0"); // other column intact
    }

    #[test]
    fn delete_cells_shift_up() {
        let mut d = DataProxy::new("t");
        d.set_cell_text(0, 0, "a0");
        d.set_cell_text(1, 0, "a1");
        d.set_cell_text(2, 0, "a2");
        d.delete_cells(0, 0, 0, 0, false);
        assert_eq!(d.get_cell_text(0, 0), "a1");
        assert_eq!(d.get_cell_text(1, 0), "a2");
        assert_eq!(d.get_cell_text(2, 0), "");
    }

    #[test]
    fn insert_cells_shift_right() {
        let mut d = DataProxy::new("t");
        d.set_cell_text(0, 0, "a");
        d.set_cell_text(0, 1, "b");
        d.set_cell_text(1, 0, "x"); // untouched row
        d.insert_cells(0, 0, 0, 0, true);
        assert_eq!(d.get_cell_text(0, 0), "");
        assert_eq!(d.get_cell_text(0, 1), "a");
        assert_eq!(d.get_cell_text(0, 2), "b");
        assert_eq!(d.get_cell_text(1, 0), "x"); // other row intact
    }

    #[test]
    fn delete_cells_shift_left() {
        let mut d = DataProxy::new("t");
        d.set_cell_text(0, 0, "a");
        d.set_cell_text(0, 1, "b");
        d.set_cell_text(0, 2, "c");
        d.delete_cells(0, 0, 0, 0, true);
        assert_eq!(d.get_cell_text(0, 0), "b");
        assert_eq!(d.get_cell_text(0, 1), "c");
        assert_eq!(d.get_cell_text(0, 2), "");
    }

    #[test]
    fn delete_cells_formula_ref_to_deleted_cell_becomes_ref() {
        // Shift up: a formula referencing a deleted cell → #REF!.
        let mut d = DataProxy::new("t");
        d.set_cell_text(0, 0, "old");
        d.set_cell_text(2, 0, "=A1"); // references the to-be-deleted cell
        d.delete_cells(0, 0, 0, 0, false); // shift up, deletes A1
        assert_eq!(d.get_cell_text(0, 0), ""); // A1 cleared, nothing above to pull up
        assert_eq!(d.get_cell_text(1, 0), "=#REF!");
        assert_eq!(d.cell_display_value(1, 0), "#REF!");
    }

    #[test]
    fn delete_cells_formula_ref_to_shifted_cell_adjusts() {
        // Shift up: a formula referencing a cell below the deleted rect → adjusted.
        let mut d = DataProxy::new("t");
        d.set_cell_text(1, 0, "val");
        d.set_cell_text(3, 0, "=A2"); // references A2 (row 1), which shifts to A1
        d.delete_cells(0, 0, 0, 0, false); // shift up, deletes A1, A2→A1, A3→A2, etc.
        assert_eq!(d.get_cell_text(0, 0), "val"); // A2 moved up to A1
        assert_eq!(d.get_cell_text(2, 0), "=A1"); // reference adjusted
    }

    #[test]
    fn delete_cells_absolute_ref_to_deleted_cell_becomes_ref() {
        // Even $A$1 becomes #REF! when A1 itself is deleted.
        let mut d = DataProxy::new("t");
        d.set_cell_text(0, 0, "pin");
        d.set_cell_text(1, 0, "=$A$1");
        d.delete_cells(0, 0, 0, 0, false);
        assert_eq!(d.get_cell_text(0, 0), "=#REF!");
        assert_eq!(d.cell_display_value(0, 0), "#REF!");
    }

    #[test]
    fn delete_cells_ref_to_cell_outside_rect_and_shift_zone_untouched() {
        // Shift up: a formula referencing a cell in a different column AND
        // outside the shift zone stays completely unchanged.
        let mut d = DataProxy::new("t");
        d.set_cell_text(0, 0, "del");
        d.set_cell_text(0, 1, "=B3"); // references B3 — col B, different from deleted col A
        d.set_cell_text(2, 1, "z"); // B3
        d.delete_cells(0, 0, 0, 0, false); // shift up, deletes A1 only (col A, row 0)
                                           // =B3 is in col B, the shifted column is A — reference B3 is untouched
        assert_eq!(d.get_cell_text(0, 1), "=B3");
        assert_eq!(d.cell_display_value(0, 1), "z");
    }

    #[test]
    fn delete_cells_locked_ref_to_shifted_cell_stays_put() {
        // $ references in the shift zone don't shift (but still #REF! if deleted).
        let mut d = DataProxy::new("t");
        d.set_cell_text(2, 0, "v0");
        d.set_cell_text(3, 0, "v1");
        d.set_cell_text(4, 0, "=$A$3"); // $A$3 (= row 2), below deleted rect, column same
        d.delete_cells(0, 0, 0, 0, false); // delete A1, shift up col A rows > 0
        assert_eq!(d.get_cell_text(3, 0), "=$A$3"); // $A$3 locked, doesn't shift
                                                    // A3 (now row 2) contains v1 (was A4, shifted up into A3)
        assert_eq!(d.cell_display_value(3, 0), "v1");
    }

    #[test]
    fn insert_cells_drops_overlapping_merge() {
        let mut d = DataProxy::new("t");
        d.merges.add(CellRange::new(0, 0, 1, 1)); // A1:B2
        assert!(d.cell_merge(0, 0).is_some());
        d.insert_cells(0, 0, 0, 0, false);
        assert!(
            d.cell_merge(0, 0).is_none(),
            "a merge overlapping the inserted band should be dropped"
        );
    }

    #[test]
    fn merge_range_sets_anchor_span_and_clears_covered() {
        let mut d = DataProxy::new("t");
        d.set_cell_text(0, 0, "anchor");
        d.set_cell_text(0, 1, "gone");
        d.set_cell_text(1, 1, "gone2");
        d.merge_range(CellRange::new(0, 0, 1, 1)); // A1:B2
                                                   // Anchor keeps its text and a (1,1) extra-span; covered cells cleared.
        assert_eq!(d.get_cell_text(0, 0), "anchor");
        assert_eq!(d.get_cell(0, 0).unwrap().merge, Some((1, 1)));
        assert_eq!(d.get_cell_text(0, 1), "");
        assert_eq!(d.get_cell_text(1, 1), "");
        assert!(
            d.cell_merge(1, 1).is_some(),
            "covered cell reports the merge"
        );
    }

    #[test]
    fn merge_range_is_a_noop_for_single_cell() {
        let mut d = DataProxy::new("t");
        d.set_cell_text(0, 0, "x");
        d.merge_range(CellRange::new(0, 0, 0, 0));
        assert_eq!(d.get_cell(0, 0).unwrap().merge, None);
        assert!(d.cell_merge(0, 0).is_none());
    }

    #[test]
    fn unmerge_intersecting_drops_partially_overlapping_merges_and_clears_anchor() {
        let mut d = DataProxy::new("t");
        d.merge_range(CellRange::new(0, 0, 1, 1)); // A1:B2
        d.merge_range(CellRange::new(5, 5, 5, 6)); // F6:G6 (untouched)
        assert!(d.cell_merge(0, 0).is_some());
        // A range that only partially overlaps A1:B2 (shares B2 only).
        d.unmerge_intersecting(&CellRange::new(1, 1, 3, 3));
        assert!(
            d.cell_merge(0, 0).is_none(),
            "partially-overlapping merge removed"
        );
        assert_eq!(
            d.get_cell(0, 0).and_then(|c| c.merge),
            None,
            "anchor marker cleared"
        );
        assert!(
            d.cell_merge(5, 5).is_some(),
            "non-intersecting merge survives"
        );
    }

    #[test]
    fn insert_cells_block_shifts_by_its_height() {
        let mut d = DataProxy::new("t");
        d.set_cell_text(0, 0, "keep");
        d.set_cell_text(3, 0, "below");
        // Insert a 3-row-tall block at rows 1..=3 → "below" moves down by 3.
        d.insert_cells(1, 0, 3, 0, false);
        assert_eq!(d.get_cell_text(0, 0), "keep"); // above the band, intact
        assert_eq!(d.get_cell_text(6, 0), "below"); // 3 + 3
    }

    // --- Sort & AutoFilter (issue #10) ---

    /// A1:B4 table — header row + 3 data rows; the filter range covers it.
    fn filter_fixture() -> DataProxy {
        let mut d = DataProxy::new("t");
        for (ri, name, score) in [(1, "carol", "33"), (2, "alice", "2"), (3, "bob", "10")] {
            d.set_cell_text(ri, 0, name);
            d.set_cell_text(ri, 1, score);
        }
        d.set_cell_text(0, 0, "Name");
        d.set_cell_text(0, 1, "Score");
        d.set_selected_range(CellRange::new(0, 0, 3, 1));
        d.autofilter(); // ref = A1:B4
        d
    }

    #[test]
    fn filter_in_hides_nonmatching_rows_and_all_reveals() {
        let mut d = filter_fixture();
        d.auto_filter
            .add_filter(0, "in", vec!["alice".into(), "bob".into()]);
        d.apply_filter_visibility();
        assert!(d.is_row_hidden(1), "carol row should hide");
        assert!(!d.is_row_hidden(2) && !d.is_row_hidden(3));
        assert!(!d.is_row_hidden(0), "header row never hides");

        d.auto_filter.add_filter(0, "all", vec![]);
        d.apply_filter_visibility();
        assert!(!d.is_row_hidden(1), "'all' reveals previously hidden rows");
    }

    #[test]
    fn toggling_autofilter_off_reveals_hidden_rows() {
        let mut d = filter_fixture();
        d.auto_filter.add_filter(0, "in", vec!["alice".into()]);
        d.apply_filter_visibility();
        assert!(d.is_row_hidden(1) && d.is_row_hidden(3));
        d.autofilter(); // toggle off
        assert!(!d.auto_filter.active());
        assert!(!d.is_row_hidden(1) && !d.is_row_hidden(3));
    }

    #[test]
    fn sort_filter_range_numeric_and_desc() {
        let mut d = filter_fixture();
        d.sort_filter_range(1, true); // by Score asc: 2, 10, 33
        assert_eq!(
            [
                d.get_cell_text(1, 1),
                d.get_cell_text(2, 1),
                d.get_cell_text(3, 1)
            ],
            ["2", "10", "33"]
        );
        // Whole data rows move together: names follow their scores.
        assert_eq!(
            [
                d.get_cell_text(1, 0),
                d.get_cell_text(2, 0),
                d.get_cell_text(3, 0)
            ],
            ["alice", "bob", "carol"]
        );
        assert_eq!(d.get_cell_text(0, 1), "Score", "header row not sorted");

        d.sort_filter_range(1, false); // desc: 33, 10, 2
        assert_eq!(
            [
                d.get_cell_text(1, 1),
                d.get_cell_text(2, 1),
                d.get_cell_text(3, 1)
            ],
            ["33", "10", "2"]
        );
        assert_eq!(
            d.auto_filter.sort.first().map(|s| s.order.as_str()),
            Some("desc")
        );
    }

    #[test]
    fn sort_leaves_columns_outside_the_range_alone() {
        let mut d = filter_fixture(); // range is A1:B4
        d.set_cell_text(1, 3, "d1"); // column D, outside
        d.set_cell_text(3, 3, "d3");
        d.sort_filter_range(0, true); // names asc reorders rows 1..=3
        assert_eq!(d.get_cell_text(1, 3), "d1", "outside column untouched");
        assert_eq!(d.get_cell_text(3, 3), "d3");
    }

    // --- Issue #2: text, criteria, and lookup functions (Value engine) ---

    /// Build a sheet from `(row, col, text)` triples and evaluate `formula`
    /// in a spare cell, returning its display value.
    fn eval_with(cells: &[(usize, usize, &str)], formula: &str) -> String {
        let mut d = DataProxy::new("t");
        for (r, c, t) in cells {
            d.set_cell_text(*r, *c, t);
        }
        d.set_cell_text(60, 60, formula);
        d.cell_display_value(60, 60)
    }

    #[test]
    fn text_functions() {
        let cells = [(0, 0, "hello"), (0, 1, "World")];
        assert_eq!(eval_with(&cells, "=UPPER(A1)"), "HELLO");
        assert_eq!(eval_with(&cells, "=LOWER(B1)"), "world");
        assert_eq!(eval_with(&cells, "=LEN(A1)"), "5");
        assert_eq!(eval_with(&cells, "=LEFT(A1, 2)"), "he");
        assert_eq!(eval_with(&cells, "=LEFT(A1)"), "h");
        assert_eq!(eval_with(&cells, "=RIGHT(A1, 3)"), "llo");
        assert_eq!(eval_with(&cells, "=MID(A1, 2, 3)"), "ell");
        assert_eq!(eval_with(&cells, "=TRIM(\"  x  \")"), "x");
        assert_eq!(eval_with(&cells, "=CONCAT(A1, \" \", B1)"), "hello World");
        assert_eq!(eval_with(&cells, "=CONCATENATE(A1, B1)"), "helloWorld");
        assert_eq!(eval_with(&cells, "=CONCAT(\"v\", 1+1)"), "v2"); // numbers coerce
        assert_eq!(eval_with(&[], "=TEXT(1234.5, \"#,##0.00\")"), "1,234.50");
    }

    #[test]
    fn string_comparisons_and_not_equal() {
        let cells = [(0, 0, "apple")];
        assert_eq!(eval_with(&cells, "=IF(A1=\"apple\", 1, 0)"), "1");
        assert_eq!(eval_with(&cells, "=IF(A1=\"APPLE\", 1, 0)"), "1"); // case-insensitive
        assert_eq!(eval_with(&cells, "=IF(A1<>\"pear\", 1, 0)"), "1");
        assert_eq!(eval_with(&cells, "=IF(A1<\"banana\", 1, 0)"), "1"); // a… < b…
        assert_eq!(eval_with(&cells, "=IF(A1=5, 1, 0)"), "0"); // text ≠ number
        assert_eq!(eval_with(&[], "=IF(2<>2, 1, 0)"), "0"); // numeric <>
    }

    #[test]
    fn iferror_recovers_from_errors() {
        let cells = [(0, 0, "10"), (0, 1, "0")];
        assert_eq!(
            eval_with(&cells, "=IFERROR(A1/B1, \"fallback\")"),
            "fallback"
        );
        assert_eq!(eval_with(&cells, "=IFERROR(A1/2, \"fallback\")"), "5");
        assert_eq!(eval_with(&cells, "=IFERROR(SQRT(-1), 42)"), "42");
    }

    #[test]
    fn countif_sumif_averageif() {
        let cells = [
            (0, 0, "apple"),
            (0, 1, "10"),
            (1, 0, "banana"),
            (1, 1, "20"),
            (2, 0, "apricot"),
            (2, 1, "30"),
            (3, 0, "banana"),
            (3, 1, "40"),
        ];
        assert_eq!(eval_with(&cells, "=COUNTIF(A1:A4, \"banana\")"), "2");
        assert_eq!(eval_with(&cells, "=COUNTIF(A1:A4, \"ap*\")"), "2"); // wildcard
        assert_eq!(eval_with(&cells, "=COUNTIF(B1:B4, \">15\")"), "3");
        assert_eq!(eval_with(&cells, "=SUMIF(B1:B4, \">15\")"), "90");
        assert_eq!(eval_with(&cells, "=SUMIF(A1:A4, \"banana\", B1:B4)"), "60");
        assert_eq!(eval_with(&cells, "=AVERAGEIF(B1:B4, \"<>10\")"), "30");
        assert_eq!(
            eval_with(&cells, "=AVERAGEIF(A1:A4, \"plum\", B1:B4)"),
            "#DIV/0!"
        );
    }

    #[test]
    fn vlookup_and_hlookup() {
        let cells = [
            (0, 0, "1"),
            (0, 1, "one"),
            (1, 0, "2"),
            (1, 1, "two"),
            (2, 0, "3"),
            (2, 1, "three"),
            // HLOOKUP table at D1:E2 — keys across the top.
            (0, 3, "x"),
            (0, 4, "y"),
            (1, 3, "ex"),
            (1, 4, "why"),
        ];
        assert_eq!(eval_with(&cells, "=VLOOKUP(2, A1:B3, 2)"), "two");
        assert_eq!(eval_with(&cells, "=VLOOKUP(2.7, A1:B3, 2)"), "two"); // approx: last ≤
        assert_eq!(eval_with(&cells, "=VLOOKUP(0.5, A1:B3, 2)"), "#N/A"); // below table
        assert_eq!(eval_with(&cells, "=VLOOKUP(2.7, A1:B3, 2, 0)"), "#N/A"); // exact mode
        assert_eq!(eval_with(&cells, "=HLOOKUP(\"y\", D1:E2, 2)"), "why");
    }

    #[test]
    fn index_and_match() {
        let cells = [
            (0, 0, "a"),
            (0, 1, "b"),
            (1, 0, "c"),
            (1, 1, "d"),
            (4, 0, "10"),
            (5, 0, "20"),
            (6, 0, "30"),
        ];
        assert_eq!(eval_with(&cells, "=INDEX(A1:B2, 2, 1)"), "c");
        assert_eq!(eval_with(&cells, "=INDEX(A5:A7, 3)"), "30"); // single column
        assert_eq!(eval_with(&cells, "=INDEX(A1:B2, 5, 1)"), "#REF!");
        assert_eq!(eval_with(&cells, "=MATCH(\"d\", A2:B2, 0)"), "2");
        assert_eq!(eval_with(&cells, "=MATCH(25, A5:A7, 1)"), "2"); // largest ≤ 25
        assert_eq!(eval_with(&cells, "=MATCH(99, A5:A7, 0)"), "#N/A");
    }

    #[test]
    fn multi_criteria_sumifs_countifs_averageifs() {
        let cells = [
            (0, 0, "apple"),
            (0, 1, "red"),
            (0, 2, "10"),
            (1, 0, "banana"),
            (1, 1, "yellow"),
            (1, 2, "20"),
            (2, 0, "apple"),
            (2, 1, "green"),
            (2, 2, "30"),
            (3, 0, "apple"),
            (3, 1, "red"),
            (3, 2, "40"),
        ];
        assert_eq!(
            eval_with(&cells, "=SUMIFS(C1:C4, A1:A4, \"apple\", B1:B4, \"red\")"),
            "50"
        );
        assert_eq!(eval_with(&cells, "=COUNTIFS(A1:A4, \"apple\")"), "3");
        assert_eq!(
            eval_with(&cells, "=COUNTIFS(A1:A4, \"apple\", C1:C4, \">15\")"),
            "2"
        );
        assert_eq!(
            eval_with(
                &cells,
                "=AVERAGEIFS(C1:C4, A1:A4, \"apple\", B1:B4, \"red\")"
            ),
            "25"
        );
        assert_eq!(
            eval_with(&cells, "=AVERAGEIFS(C1:C4, A1:A4, \"plum\")"),
            "#DIV/0!"
        );
    }

    #[test]
    fn choose_function() {
        assert_eq!(eval_with(&[], "=CHOOSE(2, \"a\", \"b\", \"c\")"), "b");
        assert_eq!(eval_with(&[], "=CHOOSE(1, 10, 20)"), "10");
        assert_eq!(eval_with(&[], "=CHOOSE(5, \"a\", \"b\")"), "#VALUE!");
    }

    #[test]
    fn xlookup_and_lookup() {
        let cells = [
            (0, 0, "1"),
            (0, 1, "one"),
            (1, 0, "2"),
            (1, 1, "two"),
            (2, 0, "3"),
            (2, 1, "three"),
        ];
        assert_eq!(eval_with(&cells, "=XLOOKUP(2, A1:A3, B1:B3)"), "two");
        assert_eq!(
            eval_with(&cells, "=XLOOKUP(9, A1:A3, B1:B3, \"missing\")"),
            "missing"
        );
        assert_eq!(eval_with(&cells, "=XLOOKUP(9, A1:A3, B1:B3)"), "#N/A");
        assert_eq!(eval_with(&cells, "=LOOKUP(2.5, A1:A3, B1:B3)"), "two"); // approx: last ≤
    }

    #[test]
    fn textjoin_substitute_replace_value() {
        // Blank-skipping is tested with empty string literals (an empty *cell*
        // resolves to 0 in this engine, not a blank).
        // 2nd arg is the ignore-empty flag (bare TRUE/FALSE aren't literals in
        // this engine — use 1/0).
        assert_eq!(
            eval_with(&[], "=TEXTJOIN(\"-\", 1, \"a\", \"\", \"c\")"),
            "a-c"
        );
        assert_eq!(eval_with(&[], "=TEXTJOIN(\", \", 0, \"x\", \"y\")"), "x, y");
        assert_eq!(
            eval_with(&[], "=SUBSTITUTE(\"a-b-c\", \"-\", \"+\")"),
            "a+b+c"
        );
        assert_eq!(
            eval_with(&[], "=SUBSTITUTE(\"a-b-c\", \"-\", \"+\", 2)"),
            "a-b+c"
        );
        assert_eq!(
            eval_with(&[], "=REPLACE(\"abcdef\", 2, 3, \"XY\")"),
            "aXYef"
        );
        assert_eq!(eval_with(&[], "=VALUE(\"123\")"), "123");
        assert_eq!(eval_with(&[], "=VALUE(\"abc\")"), "#VALUE!");
    }

    #[test]
    fn find_and_search() {
        assert_eq!(eval_with(&[], "=FIND(\"b\", \"abcabc\")"), "2");
        assert_eq!(eval_with(&[], "=FIND(\"b\", \"abcabc\", 3)"), "5"); // start past the first
        assert_eq!(eval_with(&[], "=FIND(\"z\", \"abc\")"), "#VALUE!");
        assert_eq!(eval_with(&[], "=SEARCH(\"B\", \"abc\")"), "2"); // case-insensitive
        assert_eq!(eval_with(&[], "=FIND(\"B\", \"abc\")"), "#VALUE!"); // case-sensitive
    }

    #[test]
    fn info_and_error_functions() {
        // Booleans render as 1/0 in this engine.
        let cells = [(0, 0, "5"), (0, 1, "hello")]; // A1=5, B1=hello
        assert_eq!(eval_with(&cells, "=ISNUMBER(A1)"), "1");
        assert_eq!(eval_with(&cells, "=ISNUMBER(B1)"), "0");
        assert_eq!(eval_with(&cells, "=ISTEXT(B1)"), "1");
        assert_eq!(eval_with(&cells, "=ISTEXT(A1)"), "0");
        assert_eq!(eval_with(&cells, "=ISERROR(1/0)"), "1");
        assert_eq!(eval_with(&cells, "=ISERROR(A1)"), "0");
        assert_eq!(eval_with(&cells, "=ISNA(VLOOKUP(99, A1:A2, 1, 0))"), "1");
        assert_eq!(eval_with(&cells, "=ISNA(1/0)"), "0"); // #DIV/0! is not #N/A
        assert_eq!(eval_with(&cells, "=ISERR(1/0)"), "1"); // any error except #N/A
        assert_eq!(eval_with(&cells, "=ISERR(VLOOKUP(99, A1:A2, 1, 0))"), "0");
    }

    #[test]
    fn ifna_recovers_from_na_only() {
        let cells = [(0, 0, "5")];
        assert_eq!(
            eval_with(&cells, "=IFNA(VLOOKUP(99, A1:A1, 1, 0), \"none\")"),
            "none"
        );
        assert_eq!(eval_with(&cells, "=IFNA(42, \"none\")"), "42");
        assert_eq!(eval_with(&cells, "=IFNA(1/0, \"none\")"), "#DIV/0!"); // non-#N/A propagates
    }

    #[test]
    fn formulas_returning_text_display_and_nest() {
        let cells = [(0, 0, "abc"), (1, 0, "=UPPER(A1)")];
        // A formula's text result feeds other formulas (carried as text).
        assert_eq!(eval_with(&cells, "=LEN(A2)"), "3");
        assert_eq!(eval_with(&cells, "=IF(A2=\"ABC\", 1, 0)"), "1");
        // SUM over a text cell still treats it as 0 (historic behavior).
        assert_eq!(eval_with(&cells, "=SUM(A1, 5)"), "5");
    }

    #[test]
    fn if_preserves_text_branches() {
        let cells = [(0, 0, "10")];
        assert_eq!(eval_with(&cells, "=IF(A1>5, \"yes\", \"no\")"), "yes");
        assert_eq!(eval_with(&cells, "=IF(A1>50, \"yes\", \"no\")"), "no");
        assert_eq!(eval_with(&cells, "=IF(A1>5, \"yes\")"), "yes"); // 2-arg, true
        assert_eq!(eval_with(&cells, "=IF(A1>50, \"yes\")"), "0"); // 2-arg, false → 0
        assert_eq!(eval_with(&cells, "=IF(A1>5, 1, 0)"), "1"); // numeric still works
                                                               // nested, returning a text branch
        assert_eq!(
            eval_with(&cells, "=IF(A1>5, IF(A1>8, \"big\", \"mid\"), \"small\")"),
            "big"
        );
    }

    #[test]
    fn ifs_preserves_text_and_na_on_no_match() {
        let cells = [(0, 0, "75")];
        assert_eq!(
            eval_with(&cells, "=IFS(A1>=90, \"A\", A1>=70, \"B\", A1>=0, \"C\")"),
            "B"
        );
        assert_eq!(
            eval_with(&cells, "=IFS(A1>=90, \"A\", A1>=80, \"B\")"),
            "#N/A"
        ); // no match
    }

    #[test]
    fn blank_aware_functions() {
        // A1=5, A2 empty, A3="x", A4 empty, A5=0
        let cells = [(0, 0, "5"), (2, 0, "x"), (4, 0, "0")];
        assert_eq!(eval_with(&cells, "=ISBLANK(A2)"), "1"); // an empty cell is blank
        assert_eq!(eval_with(&cells, "=ISBLANK(A1)"), "0"); // a number is not
        assert_eq!(eval_with(&cells, "=ISBLANK(A5)"), "0"); // a literal 0 is not blank
        assert_eq!(eval_with(&cells, "=ISBLANK(\"\")"), "0"); // an empty string is not blank
        assert_eq!(eval_with(&cells, "=COUNTA(A1:A5)"), "3"); // A1, A3, A5
        assert_eq!(eval_with(&cells, "=COUNTBLANK(A1:A5)"), "2"); // A2, A4
                                                                  // A blank cell is now neither a number nor text (Excel parity).
        assert_eq!(eval_with(&cells, "=ISNUMBER(A2)"), "0");
        assert_eq!(eval_with(&cells, "=ISTEXT(A2)"), "0");
    }

    #[test]
    fn blank_still_coerces_to_zero_in_math_and_compare() {
        // The Value::Blank refactor must preserve the historic "empty == 0".
        let cells = [(0, 0, "5")]; // A1=5, A2 empty
        assert_eq!(eval_with(&cells, "=A2+10"), "10");
        assert_eq!(eval_with(&cells, "=SUM(A1:A2)"), "5");
        assert_eq!(eval_with(&cells, "=IF(A2=0, \"z\", \"nz\")"), "z");
    }

    #[test]
    fn cmp_cell_values_blanks_always_last() {
        use std::cmp::Ordering;
        assert_eq!(cmp_cell_values("", "5", true), Ordering::Greater);
        assert_eq!(cmp_cell_values("", "5", false), Ordering::Greater);
        assert_eq!(cmp_cell_values("5", "", false), Ordering::Less);
        assert_eq!(cmp_cell_values("10", "9", true), Ordering::Greater); // numeric, not lexicographic
        assert_eq!(cmp_cell_values("Apple", "banana", true), Ordering::Less); // case-insensitive
    }

    // --- Conditional formatting (issue #11) ---

    fn red_rule(range: &str, op: &str, v1: &str) -> CondRule {
        CondRule {
            range: range.into(),
            op: op.into(),
            v1: v1.into(),
            v2: String::new(),
            v3: String::new(),
            bgcolor: Some("#ffc7ce".into()),
            color: Some("#9c0006".into()),
            bold: true,
        }
    }

    #[test]
    fn cond_format_overrides_style_for_matching_cells_only() {
        let mut d = DataProxy::new("t");
        d.set_cell_text(1, 1, "200");
        d.set_cell_text(2, 1, "100");
        d.set_cell_text(3, 3, "999"); // outside the rule range
        d.cond_formats.push(red_rule("B2:B3", "gt", "150"));

        let mut s = d.get_cell_style(1, 1);
        d.apply_cond_format(1, 1, &mut s);
        assert_eq!(s.bgcolor.as_deref(), Some("#ffc7ce"));
        assert_eq!(s.color, "#9c0006");
        assert!(s.bold);

        let mut s = d.get_cell_style(2, 1);
        d.apply_cond_format(2, 1, &mut s);
        assert_ne!(
            s.bgcolor.as_deref(),
            Some("#ffc7ce"),
            "100 doesn't match > 150"
        );

        let mut s = d.get_cell_style(3, 3);
        d.apply_cond_format(3, 3, &mut s);
        assert_ne!(s.bgcolor.as_deref(), Some("#ffc7ce"), "outside the range");
    }

    #[test]
    fn cond_format_matches_raw_value_not_formatted_display() {
        let mut d = DataProxy::new("t");
        d.set_cell_text(0, 0, "1234.5");
        let usd = Style {
            format: "usd".to_string(),
            ..Style::default()
        };
        let idx = d.add_style(usd);
        d.set_cell_style(0, 0, idx);
        assert_eq!(d.cell_display_value(0, 0), "$1,234.50"); // formatted
        d.cond_formats.push(red_rule("A1", "gt", "1000"));
        let mut s = d.get_cell_style(0, 0);
        d.apply_cond_format(0, 0, &mut s);
        assert_eq!(
            s.bgcolor.as_deref(),
            Some("#ffc7ce"),
            "rule sees the raw 1234.5"
        );
    }

    #[test]
    fn cond_format_first_matching_rule_wins_and_formulas_count() {
        let mut d = DataProxy::new("t");
        d.set_cell_text(0, 0, "=100+100"); // evaluates to 200
        d.cond_formats.push(red_rule("A1", "gt", "150"));
        let mut green = red_rule("A1", "gt", "10");
        green.bgcolor = Some("#c6efce".into());
        d.cond_formats.push(green);

        let mut s = d.get_cell_style(0, 0);
        d.apply_cond_format(0, 0, &mut s);
        assert_eq!(s.bgcolor.as_deref(), Some("#ffc7ce"), "first rule wins");
    }

    #[test]
    fn cond_format_scale2_interpolates_fill() {
        let mut d = DataProxy::new("t");
        for (r, v) in [(0, "0"), (1, "50"), (2, "100")] {
            d.set_cell_text(r, 0, v);
        }
        d.cond_formats.push(CondRule {
            range: "A1:A3".into(),
            op: "scale2".into(),
            v1: "#000000".into(),
            v2: "#ffffff".into(),
            v3: String::new(),
            bgcolor: None,
            color: None,
            bold: false,
        });
        let fill = |r: usize| {
            let mut s = d.get_cell_style(r, 0);
            d.apply_cond_format(r, 0, &mut s);
            s.bgcolor
        };
        assert_eq!(fill(0).as_deref(), Some("#000000")); // min
        assert_eq!(fill(1).as_deref(), Some("#808080")); // midpoint
        assert_eq!(fill(2).as_deref(), Some("#ffffff")); // max
    }

    #[test]
    fn cond_format_rules_survive_serialization_roundtrip() {
        let mut src = DataProxy::new("t");
        src.cond_formats.push(red_rule("B2:B10", "between", "10"));
        let mut dst = DataProxy::new("t");
        dst.set_data(src.get_data());
        assert_eq!(dst.cond_formats.len(), 1);
        assert_eq!(dst.cond_formats[0].range, "B2:B10");
        assert_eq!(dst.cond_formats[0].op, "between");
        assert_eq!(dst.cond_formats[0].bgcolor.as_deref(), Some("#ffc7ce"));
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
        assert_eq!(
            fill_line(&["1".into(), "2".into()], 3, true),
            vec!["3", "4", "5"]
        );
        assert_eq!(
            fill_line(&["2".into(), "4".into()], 2, true),
            vec!["6", "8"]
        );
        assert_eq!(
            fill_line(&["10".into(), "8".into()], 2, true),
            vec!["6", "4"]
        ); // descending
    }

    #[test]
    fn fill_line_single_number_copies() {
        assert_eq!(fill_line(&["5".into()], 3, true), vec!["5", "5", "5"]);
    }

    #[test]
    fn fill_line_text_copies_cyclically() {
        assert_eq!(
            fill_line(&["a".into(), "b".into()], 3, true),
            vec!["a", "b", "a"]
        );
    }

    #[test]
    fn fill_line_formula_shifts() {
        // A single formula filled down shifts one extra row per step.
        assert_eq!(
            fill_line(&["=B1".into()], 3, true),
            vec!["=B2", "=B3", "=B4"]
        );
        // Filled right shifts columns instead.
        assert_eq!(fill_line(&["=A1".into()], 2, false), vec!["=B1", "=C1"]);
    }

    #[test]
    fn fill_line_absolute_col_stays_put_when_filled_right() {
        // $A1 — column locked, row relative.
        // Filled RIGHT (axis_is_row=false → shifts columns).
        assert_eq!(fill_line(&["=$A1".into()], 2, false), vec!["=$A1", "=$A1"]);
    }

    #[test]
    fn fill_line_absolute_row_stays_put_when_filled_right() {
        // A$1 — row locked, column relative.
        // Filled RIGHT (axis_is_row=false → shifts columns, row locked stays).
        assert_eq!(fill_line(&["=A$1".into()], 2, false), vec!["=B$1", "=C$1"]);
    }

    #[test]
    fn fill_line_relative_formula_filled_down_only_shifts_rows() {
        // =A1 filled DOWN: column stays A, row shifts 1→2, 1→3, 1→4.
        assert_eq!(
            fill_line(&["=A1".into()], 3, true),
            vec!["=A2", "=A3", "=A4"]
        );
    }

    #[test]
    fn fill_line_fully_absolute_stays_put_in_both_directions() {
        // $A$1 — both locked. Neither direction changes it.
        assert_eq!(
            fill_line(&["=$A$1".into()], 2, true),
            vec!["=$A$1", "=$A$1"]
        );
        assert_eq!(
            fill_line(&["=$A$1".into()], 2, false),
            vec!["=$A$1", "=$A$1"]
        );
    }

    #[test]
    fn fill_line_mixed_absolute_in_multi_source_tiling() {
        // Two source cells: one with locked col, one with locked row.
        // Filled down (axis_is_row=true, shift=2 for first cycle).
        let src = vec!["=$A1".to_string(), "=B$1".to_string()];
        let filled = fill_line(&src, 4, true);
        // i=0: src=$A1 shift=2 → row 1→3 → $A3; i=1: src=B$1 shift=2 → col B→B, row locked → B$1
        // i=2: src=$A1 shift=4 → row 1→5 → $A5; i=3: src=B$1 shift=4 → col B→B, row locked → B$1
        assert_eq!(&filled[0], "=$A3"); // row shifted, col locked
        assert_eq!(&filled[1], "=B$1"); // row locked, stays
        assert_eq!(&filled[2], "=$A5"); // second cycle, row shifted
        assert_eq!(&filled[3], "=B$1"); // row locked, stays
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
        let (mut a, _reg) =
            two_sheet_workbook(&[(0, 0, "1"), (1, 0, "2"), (2, 0, "3"), (3, 0, "4")]);
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
        let reg: SheetsRegistry = Rc::new(RefCell::new(vec![
            DataProxy::new("S1"),
            DataProxy::new("S2"),
        ]));
        for d in reg.borrow_mut().iter_mut() {
            d.set_sheets(&reg);
        }
        let weak = Rc::downgrade(&reg);
        assert_eq!(
            Rc::strong_count(&reg),
            1,
            "back-refs must be Weak, not strong Rc"
        );
        drop(reg);
        assert!(
            weak.upgrade().is_none(),
            "workbook should free (no Rc cycle)"
        );
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
        assert_eq!(
            adjust_formula_refs("=Sheet2!A1+B1", true, 0, 1, None),
            "=Sheet2!A2+B2"
        );
        // Absolute row on the cross-sheet ref stays put.
        assert_eq!(
            adjust_formula_refs("=Sheet2!$A$1+A1", true, 0, 1, None),
            "=Sheet2!$A$1+A2"
        );
        // Both ends of a cross-sheet range shift.
        assert_eq!(
            adjust_formula_refs("=Sheet2!A1:B3", true, 0, 1, None),
            "=Sheet2!A2:B4"
        );
    }

    #[test]
    fn adjusters_leave_structured_refs_alone() {
        // `Table1` is cell-ref-shaped (letters+digits): without masking, a
        // column insert would rewrite it to `TABLF1` (issue #34). Plain refs
        // around the structured ref still shift.
        assert_eq!(
            adjust_formula_refs("=SUM(Table1[Qty])+A1", false, 0, 1, None),
            "=SUM(Table1[Qty])+B1"
        );
        assert_eq!(
            adjust_formula_refs("=Table1[[#Totals],[Q1]]", true, 0, 5, None),
            "=Table1[[#Totals],[Q1]]"
        );
        // Copy/fill keeps structured refs fixed too, like Excel.
        assert_eq!(shift_formula_refs("=[@Qty]*B1", 1, 0), "=[@Qty]*B2");
    }

    #[test]
    fn shift_formula_refs_preserves_sheet_prefix() {
        // The fill-handle shift must not corrupt cross-sheet refs.
        assert_eq!(shift_formula_refs("=Sheet2!A1", 1, 0), "=Sheet2!A2");
        assert_eq!(
            shift_formula_refs("=Sheet2!$A$1+A1", 0, 1),
            "=Sheet2!$A$1+B1"
        );
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
        let s = Style {
            rotation: Some(45.0),
            shrink_to_fit: true,
            indent: 17,
            ..Style::default()
        };
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
        d.validations.add(
            "cell",
            "C3:E5",
            Validator::new("number", false, "1,10", "be"),
        );
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
        d.validations.add(
            "cell",
            "A1",
            Validator::new("text-length", true, "1,100", "be"),
        );
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
        assert!(
            json.get("validations").is_some(),
            "validations key must serialize"
        );
        let arr = json.get("validations").unwrap().as_array().unwrap();
        assert_eq!(arr.len(), 1);
    }

    // Active cell + selection round-trip through the wire format (issue #44).
    #[test]
    fn sel_round_trips_through_json() {
        // Active cell B5 (ri=4, ci=1) with a B5:C7 selection rectangle.
        let mut d = DataProxy::new("t");
        d.selector.ri = 4;
        d.selector.ci = 1;
        d.selector.range = CellRange::new(4, 1, 6, 2);

        let json = d.get_data();
        let sel = json
            .get("sel")
            .expect("get_data must include a sel key (issue #44)");
        assert_eq!(sel.get("ri").and_then(|v| v.as_u64()), Some(4));
        assert_eq!(sel.get("ci").and_then(|v| v.as_u64()), Some(1));

        // Round-trip into a fresh sheet restores the active cell + range.
        let mut d2 = DataProxy::new("t");
        d2.set_data(json);
        assert_eq!((d2.selector.ri, d2.selector.ci), (4, 1));
        let r = &d2.selector.range;
        assert_eq!((r.sri, r.sci, r.eri, r.eci), (4, 1, 6, 2));
    }

    #[test]
    fn set_data_without_sel_keeps_default_a1() {
        // Older payloads omit `sel`; set_data must not panic and must leave the
        // default A1 selection rather than reading garbage.
        let mut json = DataProxy::new("t").get_data();
        json.as_object_mut().unwrap().remove("sel");

        let mut d = DataProxy::new("t");
        d.set_data(json);
        assert_eq!((d.selector.ri, d.selector.ci), (0, 0));
    }

    // A named range loaded from JSON with a mixed-case key must resolve inside
    // formulas, which look it up by upper-cased name (issue #45).
    #[test]
    fn json_named_range_resolves_in_formula() {
        let mut d = DataProxy::new("t");
        for (r, v) in [(0, "10"), (1, "20"), (2, "30"), (3, "30"), (4, "15")] {
            d.set_cell_text(r, 5, v); // F1:F5
        }
        // set_data only touches the keys present, so the cells above survive.
        d.set_data(serde_json::json!({ "namedRanges": { "testRange": "F1:F5" } }));
        // Stored under the upper-cased key so resolve_name hits.
        assert_eq!(d.get_named_range("testRange").as_deref(), Some("F1:F5"));
        d.set_cell_text(10, 0, "=SUM(testRange)");
        assert_eq!(d.cell_display_value(10, 0), "105");
    }

    // View zoom (issue #32): get_row_height/get_col_width are the single
    // geometry source for render AND hit-testing, so zoom applies there.
    #[test]
    fn zoom_scales_track_sizes() {
        let mut d = DataProxy::new("t");
        let (h0, w0) = (d.get_row_height(0), d.get_col_width(0));
        d.set_row_height(3, 40.0); // explicit model height
        d.set_zoom(1.5);
        assert_eq!(d.get_row_height(0), h0 * 1.5); // default height zooms
        assert_eq!(d.get_col_width(0), w0 * 1.5);
        assert_eq!(d.get_row_height(3), 60.0); // explicit height zooms
                                               // A hidden row stays collapsed at any zoom.
        d.set_row_hidden(5, true);
        assert_eq!(d.get_row_height(5), 0.0);
        // Unhiding restores the model height (zoom never corrupted it).
        d.set_row_hidden(5, false);
        assert_eq!(d.get_row_height(5), h0 * 1.5);
        // Zoom back to 100% restores original sizes exactly.
        d.set_zoom(1.0);
        assert_eq!(d.get_row_height(3), 40.0);
    }

    #[test]
    fn zoom_clamps_to_excel_range() {
        let mut d = DataProxy::new("t");
        d.set_zoom(0.01);
        assert_eq!(d.zoom(), 0.1); // 10% floor
        d.set_zoom(99.0);
        assert_eq!(d.zoom(), 4.0); // 400% ceiling
    }

    #[test]
    fn freeze_total_width_respects_zoom() {
        let mut d = DataProxy::new("t");
        d.set_freeze(0, 2);
        let w = d.freeze_total_width();
        d.set_zoom(2.0);
        assert_eq!(d.freeze_total_width(), w * 2.0);
    }

    // --- Conditional-formatting rule types (issue #29) ---

    /// Sheet with B1:B5 = 10,20,30,40,40 and a rule; returns the bgcolor that
    /// apply_cond_format leaves on each of B1..B5.
    fn cf_probe(rule: CondRule) -> Vec<Option<String>> {
        let mut d = DataProxy::new("t");
        for (r, v) in [(0, "10"), (1, "20"), (2, "30"), (3, "40"), (4, "40")] {
            d.set_cell_text(r, 1, v);
        }
        d.cond_formats.push(rule);
        (0..5)
            .map(|r| {
                let mut s = Style {
                    bgcolor: None,
                    ..Style::default()
                };
                d.apply_cond_format(r, 1, &mut s);
                s.bgcolor
            })
            .collect()
    }

    fn cf_rule(op: &str, v1: &str) -> CondRule {
        CondRule {
            range: "B1:B5".into(),
            op: op.into(),
            v1: v1.into(),
            v2: String::new(),
            v3: String::new(),
            bgcolor: Some("#ff0000".into()),
            color: None,
            bold: false,
        }
    }

    #[test]
    fn cf_top_bottom_n_with_ties_and_percent() {
        let hit = Some("#ff0000".to_string());
        // Top 2 of [10,20,30,40,40]: threshold is 40 — BOTH 40s match (ties).
        assert_eq!(
            cf_probe(cf_rule("top", "2")),
            vec![None, None, None, hit.clone(), hit.clone()]
        );
        // Bottom 2: 10 and 20.
        assert_eq!(
            cf_probe(cf_rule("bottom", "2")),
            vec![hit.clone(), hit.clone(), None, None, None]
        );
        // Top 40% of 5 values = top 2 (rounded count).
        assert_eq!(
            cf_probe(cf_rule("top", "40%")),
            vec![None, None, None, hit.clone(), hit.clone()]
        );
        // N larger than the range clamps to "everything".
        assert_eq!(
            cf_probe(cf_rule("top", "99"))
                .iter()
                .filter(|x| x.is_some())
                .count(),
            5
        );
        // Unparsable N matches nothing.
        assert_eq!(cf_probe(cf_rule("top", "x")), vec![None; 5]);
    }

    #[test]
    fn cf_above_below_average() {
        let hit = Some("#ff0000".to_string());
        // mean(10,20,30,40,40) = 28 — strictly above: 30, 40, 40.
        assert_eq!(
            cf_probe(cf_rule("above-avg", "")),
            vec![None, None, hit.clone(), hit.clone(), hit.clone()]
        );
        // Strictly below: 10, 20.
        assert_eq!(
            cf_probe(cf_rule("below-avg", "")),
            vec![hit.clone(), hit.clone(), None, None, None]
        );
    }

    #[test]
    fn cf_duplicate_and_unique_values() {
        let hit = Some("#ff0000".to_string());
        // Only the two 40s are duplicates.
        assert_eq!(
            cf_probe(cf_rule("dup", "")),
            vec![None, None, None, hit.clone(), hit.clone()]
        );
        // Everything else is unique.
        assert_eq!(
            cf_probe(cf_rule("unique", "")),
            vec![hit.clone(), hit.clone(), hit.clone(), None, None]
        );
    }

    #[test]
    fn cf_formula_rule_shifts_relative_refs() {
        let hit = Some("#ff0000".to_string());
        // Anchored at the range top-left (B1): "=B1>25" shifts per row, so it
        // matches the cells holding 30, 40, 40.
        assert_eq!(
            cf_probe(cf_rule("formula", "=B1>25")),
            vec![None, None, hit.clone(), hit.clone(), hit.clone()]
        );
        // $-anchored refs do NOT shift: =$B$1>25 is false everywhere (B1=10).
        assert_eq!(cf_probe(cf_rule("formula", "=$B$1>25")), vec![None; 5]);
        // Empty formula matches nothing.
        assert_eq!(cf_probe(cf_rule("formula", "  ")), vec![None; 5]);
    }

    #[test]
    fn cf_scale3_blends_min_mid_max() {
        let mut rule = cf_rule("scale3", "#000000");
        rule.v2 = "#808080".into();
        rule.v3 = "#ffffff".into();
        rule.bgcolor = None;
        let got = cf_probe(rule);
        // 10 → min color, 40 → max color; 30 is t=2/3 → blends mid→max.
        assert_eq!(got[0].as_deref(), Some("#000000"));
        assert_eq!(got[3].as_deref(), Some("#ffffff"));
        let mid = got[2].as_deref().unwrap();
        assert!(
            mid > "#808080" && mid < "#ffffff",
            "30 blends between mid and max: {mid}"
        );
    }

    #[test]
    fn cf_visuals_databar_and_icons() {
        let mut d = DataProxy::new("t");
        for (r, v) in [(0, "10"), (1, "25"), (2, "40")] {
            d.set_cell_text(r, 1, v);
        }
        let mut bar = cf_rule("databar", "");
        bar.range = "B1:B3".into();
        bar.bgcolor = Some("#638ec6".into());
        d.cond_formats.push(bar);
        // Min keeps a visible sliver; max fills the cell.
        match d.cond_visual(0, 1) {
            Some(CondVisual::Bar { frac, ref color }) => {
                assert!((frac - 0.05).abs() < 1e-9, "min bar frac: {frac}");
                assert_eq!(color, "#638ec6");
            }
            v => panic!("expected min bar, got {v:?}"),
        }
        match d.cond_visual(2, 1) {
            Some(CondVisual::Bar { frac, .. }) => assert!((frac - 1.0).abs() < 1e-9),
            v => panic!("expected full bar, got {v:?}"),
        }
        // Non-numeric / out-of-range cells get no visual.
        assert_eq!(d.cond_visual(4, 1), None);

        // Icons: zones by thirds of [10, 40] — 10→low, 25→mid, 40→high.
        let mut d2 = DataProxy::new("t");
        for (r, v) in [(0, "10"), (1, "25"), (2, "40")] {
            d2.set_cell_text(r, 1, v);
        }
        let mut icons = cf_rule("icons", "traffic");
        icons.range = "B1:B3".into();
        d2.cond_formats.push(icons);
        for (r, zone) in [(0, 0u8), (1, 1), (2, 2)] {
            match d2.cond_visual(r, 1) {
                Some(CondVisual::Icon {
                    set: IconSet::Traffic,
                    zone: z,
                }) => assert_eq!(z, zone, "row {r}"),
                v => panic!("expected traffic icon, got {v:?}"),
            }
        }
        // A databar/icons rule must NOT consume the style pass: a later style
        // rule still applies (visuals stack with colors).
        let mut style_rule = cf_rule("gt", "30");
        style_rule.range = "B1:B3".into();
        d2.cond_formats.push(style_rule);
        let mut s = Style::default();
        d2.apply_cond_format(2, 1, &mut s);
        assert_eq!(
            s.bgcolor.as_deref(),
            Some("#ff0000"),
            "style rule applies alongside icons"
        );
    }

    #[test]
    fn data_bar_and_icon_set_stack_on_the_same_cell() {
        // Both a databar rule and an icons rule on the same range →
        // cond_visuals returns BOTH (a bar, then an icon). The
        // renderer draws the bar first, then the icon on top, so a
        // single cell can carry both glyphs (issue #29 follow-on).
        let mut d = DataProxy::new("t");
        for (r, v) in [(0, "10"), (1, "20"), (2, "30")] {
            d.set_cell_text(r, 1, v);
        }
        let mut bar = cf_rule("databar", "");
        bar.bgcolor = Some("#638ec6".into());
        d.cond_formats.push(bar);
        let mut icons = cf_rule("icons", "traffic");
        icons.range = "B1:B3".into();
        d.cond_formats.push(icons);

        let visuals = d.cond_visuals(1, 1);
        // First visual is the bar; second is the icon (renderer
        // order: bars before icons so the icon isn't painted over).
        assert_eq!(visuals.len(), 2, "expected 2 stacked visuals");
        match &visuals[0] {
            CondVisual::Bar { frac, .. } => {
                assert!(frac > &0.05, "mid bar should be > min sliver");
                assert!(frac < &1.0, "mid bar should be < full");
            }
            v => panic!("expected bar first, got {v:?}"),
        }
        match &visuals[1] {
            CondVisual::Icon { set, .. } => assert_eq!(*set, IconSet::Traffic),
            v => panic!("expected icon second, got {v:?}"),
        }
        // cond_visual (single) still returns the first match for
        // any existing caller that only wants one.
        assert!(matches!(d.cond_visual(1, 1), Some(CondVisual::Bar { .. })));
    }

    // --- Outline groups + SUBTOTAL (issue #30) ---

    #[test]
    fn outline_toggle_hides_and_nested_expand_keeps_inner_collapsed() {
        let mut d = DataProxy::new("t");
        d.add_row_group(1, 8); // outer
        d.add_row_group(2, 4); // inner
                               // Collapse inner: rows 2..=4 hide.
        d.toggle_row_group(1);
        assert!(d.is_row_hidden(3));
        assert!(!d.is_row_hidden(5));
        // Collapse outer too: all of 1..=8 hide.
        d.toggle_row_group(0);
        assert!(d.is_row_hidden(1) && d.is_row_hidden(8));
        // Expand outer: inner is STILL collapsed, so 2..=4 stay hidden.
        d.toggle_row_group(0);
        assert!(!d.is_row_hidden(1) && !d.is_row_hidden(8));
        assert!(d.is_row_hidden(2) && d.is_row_hidden(4));
        // Ungroup everything intersecting 0..=10: rows reappear.
        d.remove_row_groups_overlapping(0, 10);
        assert!(d.row_groups.is_empty());
        assert!(!d.is_row_hidden(3));
    }

    #[test]
    fn outline_level_buttons_collapse_by_depth() {
        let mut d = DataProxy::new("t");
        d.add_row_group(1, 8); // level 1
        d.add_row_group(2, 4); // level 2
                               // Button 1: collapse levels >= 1 (everything).
        d.set_row_outline_level(1);
        assert!(d.is_row_hidden(1) && d.is_row_hidden(3));
        // Button 2: level-1 groups expand, level-2 stay collapsed.
        d.set_row_outline_level(2);
        assert!(!d.is_row_hidden(1));
        assert!(d.is_row_hidden(3));
        // Button 3 (max+1): everything expands.
        d.set_row_outline_level(3);
        assert!(!d.is_row_hidden(3));
    }

    #[test]
    fn outline_groups_survive_serialization_and_shifts() {
        let mut d = DataProxy::new("t");
        d.add_row_group(2, 4);
        d.add_col_group(1, 3);
        d.toggle_row_group(0);
        let mut d2 = DataProxy::new("t");
        d2.set_data(d.get_data());
        assert_eq!(d2.row_groups, d.row_groups);
        assert_eq!(d2.col_groups, d.col_groups);
        assert!(d2.row_groups[0].collapsed);
        assert!(d2.is_row_hidden(3), "hide flags ride with the rows");

        // Structural edits keep groups aligned.
        let mut d3 = DataProxy::new("t");
        d3.add_row_group(5, 8);
        d3.insert_row(2, 2); // insert above: group shifts down
        assert_eq!((d3.row_groups[0].start, d3.row_groups[0].end), (7, 10));
        d3.insert_row(8, 1); // insert inside: group grows
        assert_eq!((d3.row_groups[0].start, d3.row_groups[0].end), (7, 11));
        d3.delete_row(0); // delete above: shifts up
        assert_eq!((d3.row_groups[0].start, d3.row_groups[0].end), (6, 10));
        d3.delete_row(8); // delete inside: shrinks
        assert_eq!((d3.row_groups[0].start, d3.row_groups[0].end), (6, 9));
    }

    #[test]
    fn subtotal_function_variants_and_hidden_rows() {
        let mut d = DataProxy::new("t");
        for (r, v) in [(0, "10"), (1, "20"), (2, "30"), (3, "40")] {
            d.set_cell_text(r, 0, v); // A1:A4
        }
        d.set_cell_text(4, 0, "label"); // text counts for COUNTA only
        d.set_cell_text(10, 0, "=SUBTOTAL(9, A1:A5)");
        assert_eq!(d.cell_display_value(10, 0), "100");
        // Hide row 2 (value 20): 9 still includes it, 109 skips it.
        d.set_row_hidden(1, true);
        d.set_cell_text(11, 0, "=SUBTOTAL(109, A1:A5)");
        assert_eq!(d.cell_display_value(11, 0), "80");
        d.set_cell_text(12, 0, "=SUBTOTAL(9, A1:A5)");
        assert_eq!(d.cell_display_value(12, 0), "100");
        // Other function numbers (over the visible 10, 30, 40).
        for (f, want) in [
            (101, "26.666666666666668"), // average of 10,30,40 — wait, format
            (102, "3"),                  // numeric count
            (103, "4"),                  // non-blank count (incl. "label")
            (104, "40"),
            (105, "10"),
            (106, "12000"),
            (109, "80"),
        ] {
            d.set_cell_text(20, 0, &format!("=SUBTOTAL({f}, A1:A5)"));
            let got = d.cell_display_value(20, 0);
            if f == 101 {
                assert!(got.starts_with("26.6666"), "avg: {got}");
            } else {
                assert_eq!(got, want, "fn {f}");
            }
        }
        // Variance/stdev over 1..4 (unhide first): var-sample = 5/3.
        d.set_row_hidden(1, false);
        let mut e = DataProxy::new("t");
        for (r, v) in [(0, "1"), (1, "2"), (2, "3"), (3, "4")] {
            e.set_cell_text(r, 0, v);
        }
        e.set_cell_text(10, 0, "=SUBTOTAL(10, A1:A4)");
        assert!(
            e.cell_display_value(10, 0).starts_with("1.6666"),
            "sample var"
        );
        e.set_cell_text(11, 0, "=SUBTOTAL(11, A1:A4)");
        assert_eq!(e.cell_display_value(11, 0), "1.25"); // population var
        e.set_cell_text(12, 0, "=SUBTOTAL(8, A1:A4)");
        assert!(
            e.cell_display_value(12, 0).starts_with("1.118"),
            "pop stdev"
        );
        // Bad function number.
        e.set_cell_text(13, 0, "=SUBTOTAL(42, A1:A4)");
        assert_eq!(e.cell_display_value(13, 0), "#VALUE!");
        // Composes inside arithmetic.
        e.set_cell_text(14, 0, "=SUBTOTAL(9, A1:A4) * 2");
        assert_eq!(e.cell_display_value(14, 0), "20");
    }

    #[test]
    fn subtotal_range_inserts_grouped_total_rows() {
        let mut d = DataProxy::new("t");
        // Key column A, values in B: two blocks (x: rows 0-1, y: rows 2-3).
        for (r, k, v) in [(0, "x", "1"), (1, "x", "2"), (2, "y", "3"), (3, "y", "4")] {
            d.set_cell_text(r, 0, k);
            d.set_cell_text(r, 1, v);
        }
        d.subtotal_range(0, 0, 3, 1);
        // Block 1 total row inserted at row 2; block 2 (shifted to 3..4) total at 5.
        assert_eq!(d.get_cell_text(2, 0), "x Total");
        assert_eq!(d.get_cell_text(2, 1), "=SUBTOTAL(9,B1:B2)");
        assert_eq!(d.cell_display_value(2, 1), "3");
        assert_eq!(d.get_cell_text(5, 0), "y Total");
        assert_eq!(d.get_cell_text(5, 1), "=SUBTOTAL(9,B4:B5)");
        assert_eq!(d.cell_display_value(5, 1), "7");
        // Each block became a collapsible group.
        assert_eq!(d.row_groups.len(), 2);
        assert_eq!((d.row_groups[0].start, d.row_groups[0].end), (0, 1));
        assert_eq!((d.row_groups[1].start, d.row_groups[1].end), (3, 4));
        // Collapsing a block keeps its SUBTOTAL row visible.
        d.toggle_row_group(0);
        assert!(d.is_row_hidden(0) && d.is_row_hidden(1));
        assert!(!d.is_row_hidden(2));
    }

    // End-key target: rightmost non-empty column in a row (issue #41).
    #[test]
    fn row_last_filled_col_finds_rightmost_nonempty() {
        let mut d = DataProxy::new("t");
        d.set_cell_text(2, 0, "a");
        d.set_cell_text(2, 3, "b"); // D3
        assert_eq!(d.row_last_filled_col(2), 3);
        // An empty row reports column 0 (Home/End collapse to A there).
        assert_eq!(d.row_last_filled_col(7), 0);
    }

    // --- Dynamic arrays & spill (issue #33) ---

    #[test]
    fn sequence_spills_into_neighbors() {
        let mut d = DataProxy::new("t");
        d.set_cell_text(0, 0, "=SEQUENCE(3,2,10,5)");
        // Anchor shows the top-left; the rest spills row-major.
        assert_eq!(d.cell_display_value(0, 0), "10");
        assert_eq!(d.cell_display_value(0, 1), "15");
        assert_eq!(d.cell_display_value(1, 0), "20");
        assert_eq!(d.cell_display_value(1, 1), "25");
        assert_eq!(d.cell_display_value(2, 0), "30");
        assert_eq!(d.cell_display_value(2, 1), "35");
        // Spilled cells stay empty in the model (only the anchor has text).
        assert_eq!(d.get_cell_text(1, 0), "");
        // The renderer sees one A1:B3 outline.
        let ranges = d.spill_ranges();
        assert_eq!(ranges.len(), 1);
        assert_eq!(
            (ranges[0].sri, ranges[0].sci, ranges[0].eri, ranges[0].eci),
            (0, 0, 2, 1)
        );
    }

    #[test]
    fn blocked_spill_shows_spill_error_and_recovers() {
        let mut d = DataProxy::new("t");
        d.set_cell_text(1, 0, "x"); // obstructs A2
        d.set_cell_text(0, 0, "=SEQUENCE(3)");
        assert_eq!(d.cell_display_value(0, 0), "#SPILL!");
        // The obstructing cell keeps its own value.
        assert_eq!(d.cell_display_value(1, 0), "x");
        // A reference to a blocked anchor propagates the error.
        d.set_cell_text(0, 5, "=A1+1");
        assert_eq!(d.cell_display_value(0, 5), "#SPILL!");
        // Clearing the obstruction lets the array spill (mutation marks the
        // cache dirty).
        d.set_cell_text(1, 0, "");
        assert_eq!(d.cell_display_value(0, 0), "1");
        assert_eq!(d.cell_display_value(1, 0), "2");
        assert_eq!(d.cell_display_value(2, 0), "3");
        assert_eq!(d.cell_display_value(0, 5), "2");
    }

    #[test]
    fn references_and_ranges_see_spilled_values() {
        let mut d = DataProxy::new("t");
        d.set_cell_text(0, 0, "=SEQUENCE(3)"); // A1:A3 = 1,2,3
        d.set_cell_text(0, 2, "=A2*10"); // spilled cell by direct ref
        assert_eq!(d.cell_display_value(0, 2), "20");
        d.set_cell_text(1, 2, "=SUM(A1:A3)"); // range over the spill
        assert_eq!(d.cell_display_value(1, 2), "6");
        // Spilled cells aren't blank (matching Excel).
        d.set_cell_text(2, 2, "=COUNTBLANK(A1:A3)");
        assert_eq!(d.cell_display_value(2, 2), "0");
    }

    #[test]
    fn filter_with_broadcast_comparison() {
        let cells: Vec<(usize, usize, &str)> =
            vec![(0, 0, "1"), (1, 0, "5"), (2, 0, "3"), (3, 0, "8")];
        // FILTER keeps the rows where the broadcast comparison is true; the
        // nested array collapses through SUM.
        assert_eq!(eval_with(&cells, "=SUM(FILTER(A1:A4, A1:A4>2))"), "16");
        // No matches: the if_empty argument, else #CALC!.
        assert_eq!(eval_with(&cells, "=FILTER(A1:A4, A1:A4>9, 0)"), "0");
        assert_eq!(eval_with(&cells, "=FILTER(A1:A4, A1:A4>9)"), "#CALC!");
        // Shape mismatch between array and include is #VALUE!.
        assert_eq!(eval_with(&cells, "=FILTER(A1:A4, A1:A3>2)"), "#VALUE!");
    }

    #[test]
    fn filter_spills_matching_rows() {
        let mut d = DataProxy::new("t");
        for (r, v) in [(0, "1"), (1, "5"), (2, "3"), (3, "8")] {
            d.set_cell_text(r, 0, v);
        }
        d.set_cell_text(0, 2, "=FILTER(A1:A4, A1:A4>2)");
        assert_eq!(d.cell_display_value(0, 2), "5");
        assert_eq!(d.cell_display_value(1, 2), "3");
        assert_eq!(d.cell_display_value(2, 2), "8");
    }

    #[test]
    fn sort_orders_rows_and_columns() {
        let cells: Vec<(usize, usize, &str)> = vec![
            (0, 0, "b"),
            (0, 1, "2"),
            (1, 0, "c"),
            (1, 1, "3"),
            (2, 0, "a"),
            (2, 1, "1"),
        ];
        // Default: ascending by the first column; rows travel together.
        assert_eq!(eval_with(&cells, "=INDEX(SORT(A1:B3), 1, 2)"), "1");
        // Descending by column 2.
        assert_eq!(eval_with(&cells, "=INDEX(SORT(A1:B3, 2, -1), 1, 1)"), "c");
        // sort_index out of range is #VALUE!.
        assert_eq!(eval_with(&cells, "=SORT(A1:B3, 5)"), "#VALUE!");
    }

    #[test]
    fn sort_rejects_non_finite_sort_index() {
        // Regression for issue #55: a non-finite sort_index used to
        // pass the `i < 1.0` check (NaN < 1.0 is false), saturate to
        // 0 via `as usize`, and then underflow on the `0_usize - 1`
        // line — debug-panicking and out-of-bounds in release.
        // The user-side path: =SORT(A1:B3, B1) where B1 is "NaN"
        // (a plausible action — looking up the sort column from
        // another cell).
        let cells: Vec<(usize, usize, &str)> =
            vec![(0, 0, "b"), (0, 1, "NaN"), (1, 0, "c"), (2, 0, "a")];
        // Per-cell parse of "NaN" → f64::NAN, which should now be
        // caught by the `is_finite()` guard and surfaced as #VALUE!.
        assert_eq!(eval_with(&cells, "=SORT(A1:A3, B1)"), "#VALUE!");
    }

    #[test]
    fn sortby_uses_parallel_key_arrays() {
        let cells: Vec<(usize, usize, &str)> = vec![
            (0, 0, "apple"),
            (0, 1, "3"),
            (1, 0, "pear"),
            (1, 1, "1"),
            (2, 0, "plum"),
            (2, 1, "2"),
        ];
        // Sort names by the weight column, descending.
        assert_eq!(
            eval_with(&cells, "=INDEX(SORTBY(A1:A3, B1:B3, -1), 1, 1)"),
            "apple"
        );
        assert_eq!(
            eval_with(&cells, "=INDEX(SORTBY(A1:A3, B1:B3), 1, 1)"),
            "pear"
        );
        // Key array of the wrong length is #VALUE!.
        assert_eq!(eval_with(&cells, "=SORTBY(A1:A3, B1:B2)"), "#VALUE!");
    }

    #[test]
    fn unique_dedupes_and_exactly_once() {
        let cells: Vec<(usize, usize, &str)> = vec![
            (0, 0, "a"),
            (1, 0, "B"),
            (2, 0, "b"), // same as B, case-insensitively
            (3, 0, "c"),
        ];
        assert_eq!(eval_with(&cells, "=COUNTA(UNIQUE(A1:A4))"), "3");
        // exactly_once keeps only the values that never repeat.
        assert_eq!(
            eval_with(&cells, "=COUNTA(UNIQUE(A1:A4, FALSE, TRUE))"),
            "2"
        );
        assert_eq!(eval_with(&cells, "=UNIQUE(A1:A4, FALSE, TRUE)"), "a");
    }

    #[test]
    fn randarray_respects_bounds_and_shape() {
        let mut d = DataProxy::new("t");
        d.set_cell_text(0, 0, "=RANDARRAY(2, 2, 5, 6, TRUE)");
        for r in 0..2 {
            for c in 0..2 {
                let v: f64 = d.cell_display_value(r, c).parse().unwrap();
                assert!(v == 5.0 || v == 6.0, "out of range: {v}");
            }
        }
        // min > max is #VALUE!.
        d.set_cell_text(5, 5, "=RANDARRAY(1, 1, 9, 1)");
        assert_eq!(d.cell_display_value(5, 5), "#VALUE!");
    }

    #[test]
    fn top_level_range_and_arithmetic_spill() {
        let mut d = DataProxy::new("t");
        d.set_cell_text(0, 0, "2");
        d.set_cell_text(1, 0, "4");
        // A bare range spills…
        d.set_cell_text(0, 2, "=A1:A2");
        assert_eq!(d.cell_display_value(0, 2), "2");
        assert_eq!(d.cell_display_value(1, 2), "4");
        // …and so does broadcast arithmetic over one.
        d.set_cell_text(0, 4, "=A1:A2*10+1");
        assert_eq!(d.cell_display_value(0, 4), "21");
        assert_eq!(d.cell_display_value(1, 4), "41");
        // Range-with-operators inside an argument broadcasts too.
        d.set_cell_text(5, 0, "=SUM(A1:A2*10)");
        assert_eq!(d.cell_display_value(5, 0), "60");
    }

    #[test]
    fn broadcast_per_cell_errors_spill_per_cell() {
        // Regression for issue #56: =A1:A3/0 used to collapse to a single
        // #DIV/0! at the anchor. Now each cell in the broadcast result
        // is its own per-cell #DIV/0! (Excel behavior).
        let mut d = DataProxy::new("t");
        d.set_cell_text(0, 0, "10");
        d.set_cell_text(1, 0, "20");
        d.set_cell_text(2, 0, "30");
        d.set_cell_text(0, 2, "=A1:A3/0");
        // The spill renders per-cell #DIV/0! at every spilled cell.
        assert_eq!(d.cell_display_value(0, 2), "#DIV/0!");
        assert_eq!(d.cell_display_value(1, 2), "#DIV/0!");
        assert_eq!(d.cell_display_value(2, 2), "#DIV/0!");
    }

    #[test]
    fn broadcast_mixed_errors_spill_per_cell() {
        // A mixed broadcast: 10/0 = #DIV/0!, 20/2 = 10. Per-cell errors
        // don't poison the whole expression.
        let mut d = DataProxy::new("t");
        d.set_cell_text(0, 0, "10");
        d.set_cell_text(1, 0, "20");
        d.set_cell_text(0, 1, "0");
        d.set_cell_text(1, 1, "2");
        d.set_cell_text(0, 2, "=A1:A2/B1:B2");
        assert_eq!(d.cell_display_value(0, 2), "#DIV/0!");
        assert_eq!(d.cell_display_value(1, 2), "10");
    }

    #[test]
    fn structural_edits_move_spills() {
        let mut d = DataProxy::new("t");
        d.set_cell_text(0, 0, "=SEQUENCE(2)");
        assert_eq!(d.cell_display_value(1, 0), "2");
        // Inserting a row above shifts the whole spill down (the formula
        // reference adjuster rewrites nothing here; the anchor just moves).
        d.insert_row(0, 1);
        assert_eq!(d.cell_display_value(0, 0), "");
        assert_eq!(d.cell_display_value(1, 0), "1");
        assert_eq!(d.cell_display_value(2, 0), "2");
    }

    #[test]
    fn merged_cells_block_spills() {
        let mut d = DataProxy::new("t");
        d.merge_range(CellRange::new(1, 0, 1, 1)); // A2:B2 merged
        d.set_cell_text(0, 0, "=SEQUENCE(3)");
        assert_eq!(d.cell_display_value(0, 0), "#SPILL!");
    }

    // --- Excel-style tables & structured references (issue #34) ---

    /// A 3-column sales table at A1:C4: headers Item/Qty/Price, three data
    /// rows.
    fn sales_table(d: &mut DataProxy) -> String {
        for (r, item, qty, price) in [
            (0, "Item", "Qty", "Price"),
            (1, "pen", "2", "10"),
            (2, "book", "1", "25"),
            (3, "ink", "4", "5"),
        ] {
            d.set_cell_text(r, 0, item);
            d.set_cell_text(r, 1, qty);
            d.set_cell_text(r, 2, price);
        }
        d.format_as_table(&CellRange::new(0, 0, 3, 2))
    }

    #[test]
    fn format_as_table_names_headers_and_autofilter() {
        let mut d = DataProxy::new("t");
        d.set_cell_text(0, 0, "Name"); // B1 header left empty on purpose
        let name = d.format_as_table(&CellRange::new(0, 0, 2, 1));
        assert_eq!(name, "Table1");
        // Empty header cells are filled with generated names.
        assert_eq!(d.get_cell_text(0, 1), "Column2");
        // Header dropdowns: the autofilter is pointed at the table.
        assert!(d.auto_filter.active());
        // Overlapping ranges refuse to nest and return the existing table.
        assert_eq!(d.format_as_table(&CellRange::new(1, 0, 5, 5)), "Table1");
        // A second, disjoint table gets the next free name.
        assert_eq!(d.format_as_table(&CellRange::new(10, 0, 12, 1)), "Table2");
    }

    #[test]
    fn structured_refs_resolve_columns_items_and_errors() {
        let mut d = DataProxy::new("t");
        sales_table(&mut d);
        d.set_cell_text(10, 0, "=SUM(Table1[Qty])");
        assert_eq!(d.cell_display_value(10, 0), "7");
        // Empty spec = data body; #All adds the header row.
        d.set_cell_text(10, 1, "=COUNTA(Table1[])");
        assert_eq!(d.cell_display_value(10, 1), "9");
        d.set_cell_text(10, 2, "=COUNTA(Table1[#All])");
        assert_eq!(d.cell_display_value(10, 2), "12");
        d.set_cell_text(10, 3, "=COUNTA(Table1[#Headers])");
        assert_eq!(d.cell_display_value(10, 3), "3");
        // Column names match case-insensitively; unknown ones are #REF!.
        d.set_cell_text(11, 0, "=SUM(Table1[qty])");
        assert_eq!(d.cell_display_value(11, 0), "7");
        d.set_cell_text(11, 1, "=SUM(Table1[Nope])");
        assert_eq!(d.cell_display_value(11, 1), "#REF!");
        d.set_cell_text(11, 2, "=SUM(NoTable[Qty])");
        assert_eq!(d.cell_display_value(11, 2), "#REF!");
        // No totals row yet → #Totals is #REF!.
        d.set_cell_text(11, 3, "=SUM(Table1[#Totals])");
        assert_eq!(d.cell_display_value(11, 3), "#REF!");
        // Structured refs broadcast like ranges (issue #33).
        d.set_cell_text(12, 0, "=SUM(Table1[Qty]*Table1[Price])");
        assert_eq!(d.cell_display_value(12, 0), "65");
    }

    #[test]
    fn this_row_reference_intersects_current_row() {
        let mut d = DataProxy::new("t");
        sales_table(&mut d);
        // The bare shorthand only exists inside the table's own cells.
        d.set_cell_text(2, 1, "=[@Price]*3"); // book row, price 25
        assert_eq!(d.cell_display_value(2, 1), "75");
        // The named form works from any cell on the sheet, using the
        // formula's row.
        d.set_cell_text(3, 4, "=Table1[@Qty]*Table1[@Price]");
        assert_eq!(d.cell_display_value(3, 4), "20");
        // `[@…]` outside the table's data rows is #VALUE!.
        d.set_cell_text(20, 0, "=Table1[@Qty]");
        assert_eq!(d.cell_display_value(20, 0), "#VALUE!");
        // Bare shorthand outside any table has nothing to bind to: #REF!.
        d.set_cell_text(20, 1, "=[@Qty]");
        assert_eq!(d.cell_display_value(20, 1), "#REF!");
    }

    #[test]
    fn table_column_spills_as_dynamic_array() {
        let mut d = DataProxy::new("t");
        sales_table(&mut d);
        d.set_cell_text(0, 5, "=Table1[Qty]");
        assert_eq!(d.cell_display_value(0, 5), "2");
        assert_eq!(d.cell_display_value(1, 5), "1");
        assert_eq!(d.cell_display_value(2, 5), "4");
    }

    #[test]
    fn totals_row_toggles_and_resolves() {
        let mut d = DataProxy::new("t");
        sales_table(&mut d);
        d.toggle_table_totals("Table1");
        assert_eq!(d.get_cell_text(4, 0), "Total");
        assert_eq!(d.get_cell_text(4, 2), "=SUBTOTAL(9,C2:C4)");
        assert_eq!(d.cell_display_value(4, 2), "40");
        d.set_cell_text(10, 0, "=SUM(Table1[#Totals])");
        assert_eq!(d.cell_display_value(10, 0), "40");
        // The data body still excludes the totals row.
        d.set_cell_text(10, 1, "=SUM(Table1[Price])");
        assert_eq!(d.cell_display_value(10, 1), "40");
        // The bracketed combo addresses one column of the totals row.
        d.set_cell_text(10, 2, "=Table1[[#Totals],[Price]]");
        assert_eq!(d.cell_display_value(10, 2), "40");
        // Toggling off clears the cells and shrinks the table.
        d.toggle_table_totals("Table1");
        assert_eq!(d.get_cell_text(4, 0), "");
        assert_eq!(d.tables[0].eri, 3);
    }

    #[test]
    fn typing_adjacent_expands_the_table() {
        let mut d = DataProxy::new("t");
        sales_table(&mut d);
        // Below the body: table grows a row, and the new row joins [@…] math.
        d.set_cell_text(4, 0, "pad");
        d.maybe_expand_tables(4, 0);
        assert_eq!(d.tables[0].eri, 4);
        d.set_cell_text(4, 1, "10");
        d.set_cell_text(10, 0, "=SUM(Table1[Qty])");
        assert_eq!(d.cell_display_value(10, 0), "17");
        // Right of the table: grows a column and generates its header.
        d.set_cell_text(2, 3, "x");
        d.maybe_expand_tables(2, 3);
        assert_eq!(d.tables[0].eci, 3);
        assert_eq!(d.get_cell_text(0, 3), "Column4");
        // A cell that isn't adjacent leaves the table alone.
        d.set_cell_text(20, 20, "far");
        d.maybe_expand_tables(20, 20);
        assert_eq!((d.tables[0].eri, d.tables[0].eci), (4, 3));
    }

    #[test]
    fn table_style_header_banding_and_explicit_fill() {
        let mut d = DataProxy::new("t");
        sales_table(&mut d);
        let mut style = Style::default();
        d.apply_table_style(0, 0, &mut style);
        assert_eq!(style.bgcolor.as_deref(), Some("#4472c4"));
        assert!(style.bold);
        // First body row plain, second banded.
        let mut s1 = Style::default();
        d.apply_table_style(1, 0, &mut s1);
        assert_eq!(s1.bgcolor.as_deref(), Some("#ffffff"));
        let mut s2 = Style::default();
        d.apply_table_style(2, 0, &mut s2);
        assert_eq!(s2.bgcolor.as_deref(), Some("#d9e1f2"));
        // An explicit fill on a banded row survives.
        let mut s3 = Style {
            bgcolor: Some("#ff0000".to_string()),
            ..Style::default()
        };
        d.apply_table_style(2, 0, &mut s3);
        assert_eq!(s3.bgcolor.as_deref(), Some("#ff0000"));
        // Outside the table: untouched.
        let mut s4 = Style::default();
        d.apply_table_style(20, 20, &mut s4);
        assert_eq!(s4.bgcolor.as_deref(), Some("#ffffff"));
    }

    #[test]
    fn tables_roundtrip_through_workbook_json() {
        let mut d = DataProxy::new("t");
        sales_table(&mut d);
        d.toggle_table_totals("Table1");
        let json = crate::core::workbook::serialize(&[d]);
        let sheets = crate::core::workbook::deserialize(&json);
        assert_eq!(sheets[0].tables.len(), 1);
        let t = &sheets[0].tables[0];
        assert_eq!(t.name, "Table1");
        assert_eq!((t.sri, t.sci, t.eri, t.eci), (0, 0, 4, 2));
        assert!(t.totals_row && t.banded);
        // Structured refs still resolve after the round-trip.
        let mut s = sheets.into_iter().next().unwrap();
        s.set_cell_text(10, 0, "=SUM(Table1[Qty])");
        assert_eq!(s.cell_display_value(10, 0), "7");
    }

    #[test]
    fn convert_to_range_drops_table_and_filter() {
        let mut d = DataProxy::new("t");
        sales_table(&mut d);
        d.convert_table_to_range("Table1");
        assert!(d.tables.is_empty());
        assert!(!d.auto_filter.active());
        // Cells stay; structured refs are now #REF!.
        assert_eq!(d.get_cell_text(1, 0), "pen");
        d.set_cell_text(10, 0, "=SUM(Table1[Qty])");
        assert_eq!(d.cell_display_value(10, 0), "#REF!");
    }

    #[test]
    fn structural_edits_shift_tables() {
        let mut d = DataProxy::new("t");
        sales_table(&mut d);
        d.insert_row(0, 2);
        assert_eq!((d.tables[0].sri, d.tables[0].eri), (2, 5));
        d.set_cell_text(10, 0, "=SUM(Table1[Qty])");
        assert_eq!(d.cell_display_value(10, 0), "7");
        d.delete_row(3); // first data row ("pen") — the probe shifts to row 9
        assert_eq!((d.tables[0].sri, d.tables[0].eri), (2, 4));
        assert_eq!(d.cell_display_value(9, 0), "5");
        d.insert_col(0, 1); // …and right to column 1
        assert_eq!((d.tables[0].sci, d.tables[0].eci), (1, 3));
        assert_eq!(d.cell_display_value(9, 1), "5");
    }

    #[test]
    fn spill_collision_first_anchor_wins() {
        let mut d = DataProxy::new("t");
        // Both want A2; the row-major-first anchor (A1) spills, B1's range
        // (B1:B2 then A2? no — B1 spills B1:B2) — use overlapping columns.
        d.set_cell_text(0, 0, "=SEQUENCE(3)"); // A1:A3
        d.set_cell_text(1, 1, "=SEQUENCE(1,2)"); // wants B2:C2 — free
        assert_eq!(d.cell_display_value(1, 1), "1");
        assert_eq!(d.cell_display_value(1, 2), "2");
        // A later anchor whose range hits an existing spill blocks.
        d.set_cell_text(0, 1, "=SEQUENCE(2)"); // wants B1:B2, but B2 is taken
        assert_eq!(d.cell_display_value(0, 1), "#SPILL!");
    }

    // -- Financial function tests --

    #[test]
    fn pmt_loan_payment() {
        // $100k loan at 5% APR, 30 years monthly → ~$536.82
        let r = pmt(0.05 / 12.0, 360.0, 100000.0, 0.0, 0.0);
        assert!((r + 536.82).abs() < 0.5);
    }

    #[test]
    fn pmt_with_fv_and_type() {
        // $10k at 6% for 3 years, FV=$500, type=1 (beginning)
        let r = pmt(0.06, 3.0, 10000.0, 500.0, 1.0);
        assert!((r.abs() - 3686.0).abs() < 10.0);
    }

    #[test]
    fn pv_present_value() {
        // PV of $100/month for 60 months at 0.5%/month
        let r = pv(0.005, 60.0, -100.0, 0.0, 0.0);
        assert!((r - 5172.56).abs() < 1.0);
    }

    #[test]
    fn fv_future_value() {
        // $200/month for 10 years at 5% annually (monthly compounding)
        let r = fv(0.05 / 12.0, 120.0, -200.0, 0.0, 0.0);
        assert!((r - 31057.0).abs() < 100.0);
    }

    #[test]
    fn npv_net_present_value() {
        // NPV(0.1, 300, 400, 500, 600) — cash flows from period 1 onward
        let r = npv(0.1, &[300.0, 400.0, 500.0, 600.0]);
        assert!((r - 1389.0).abs() < 10.0, "got {}", r);
    }

    #[test]
    fn irr_internal_rate() {
        let r = irr(&[-1000.0, 300.0, 400.0, 500.0, 600.0], 0.1);
        assert!((r - 0.22).abs() < 0.05);
    }

    #[test]
    fn sln_straight_line_dep() {
        // Cost $10k, salvage $1k, 5 years → $1800/year
        let r = sln(10000.0, 1000.0, 5.0);
        assert!((r - 1800.0).abs() < 0.01);
    }

    #[test]
    fn ddb_double_declining() {
        // Cost $10k, salvage $1k, 5 years, period 1
        let r = ddb(10000.0, 1000.0, 5.0, 1.0, 2.0);
        assert!((r - 4000.0).abs() < 0.01);
    }

    // -- Statistical function tests --

    #[test]
    fn stdev_p_population() {
        let data = [2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0];
        let sd = population_variance(&data).sqrt();
        assert!((sd - 2.0).abs() < 0.01);
    }

    #[test]
    fn percentile_inc_median() {
        let data = [1.0, 2.0, 3.0, 4.0, 5.0];
        let p50 = percentile_inc(&data, 0.5);
        assert!((p50 - 3.0).abs() < 0.01);
    }

    #[test]
    fn rank_eq_descending() {
        // rank of 7 → 4th (10=1, 9=2, 8=3, 7=4); RANK.EQ expects [value, data...] in flattened args
        let all = [7.0, 10.0, 7.0, 8.0, 9.0];
        let r = rank_eq(&all, 7.0);
        assert!((r - 4.0).abs() < 0.01);
    }

    // -- Formula-level integration tests --

    #[test]
    fn formula_pmt_evaluates() {
        let mut d = DataProxy::new("t");
        d.set_cell_text(0, 0, "0.05"); // A1 = rate
        d.set_cell_text(1, 0, "360"); // A2 = nper
        d.set_cell_text(2, 0, "100000"); // A3 = pv
        d.set_cell_text(3, 0, "=PMT(A1/12, A2, A3)");
        let v = d.cell_display_value(3, 0);
        let n: f64 = v.parse().unwrap();
        assert!(n < -400.0 && n > -600.0, "PMT = {}", n);
    }

    #[test]
    fn formula_npv_evaluates() {
        let mut d = DataProxy::new("t");
        d.set_cell_text(0, 0, "0.1"); // A1 = rate
        d.set_cell_text(0, 1, "100"); // B1
        d.set_cell_text(0, 2, "200"); // C1
        d.set_cell_text(0, 3, "300"); // D1
        d.set_cell_text(1, 0, "=NPV(A1, B1, C1, D1)");
        let v = d.cell_display_value(1, 0);
        let n: f64 = v.parse().unwrap();
        assert!(n > 400.0 && n < 550.0, "NPV = {}", n);
    }

    #[test]
    fn formula_sln_evaluates() {
        let mut d = DataProxy::new("t");
        d.set_cell_text(0, 0, "=SLN(10000, 1000, 5)");
        assert_eq!(d.cell_display_value(0, 0), "1800");
    }

    #[test]
    fn formula_stdev_p_evaluates() {
        let mut d = DataProxy::new("t");
        d.set_cell_text(0, 0, "2"); // A1
        d.set_cell_text(0, 1, "4"); // B1
        d.set_cell_text(0, 2, "6"); // C1
        d.set_cell_text(1, 0, "=STDEV.P(A1:C1)");
        let v = d.cell_display_value(1, 0);
        assert!(
            v != "#NAME?" && v != "#VALUE!" && v != "0",
            "unexpected: {}",
            v
        );
        let n: f64 = v.parse().unwrap();
        assert!(n > 1.0 && n < 3.0, "STDEV.P = {}", n);
    }

    #[test]
    fn type_function() {
        let mut d = DataProxy::new("t");
        d.set_cell_text(0, 0, "42");
        d.set_cell_text(0, 1, "hello");
        d.set_cell_text(1, 0, "=TYPE(A1)");
        d.set_cell_text(1, 1, "=TYPE(B1)");
        assert_eq!(d.cell_display_value(1, 0), "1"); // number
        assert_eq!(d.cell_display_value(1, 1), "2"); // text
    }

    #[test]
    fn n_function_converts_to_number() {
        let mut d = DataProxy::new("t");
        d.set_cell_text(0, 0, "hello");
        d.set_cell_text(0, 1, "42");
        d.set_cell_text(1, 0, "=N(A1)");
        d.set_cell_text(1, 1, "=N(B1)");
        assert_eq!(d.cell_display_value(1, 0), "0"); // text → 0
        assert_eq!(d.cell_display_value(1, 1), "42"); // numeric text → 42
    }

    #[test]
    fn t_function_returns_text() {
        let mut d = DataProxy::new("t");
        d.set_cell_text(0, 0, "hello");
        d.set_cell_text(0, 1, "42");
        d.set_cell_text(1, 0, "=T(A1)");
        d.set_cell_text(1, 1, "=T(B1)");
        assert_eq!(d.cell_display_value(1, 0), "hello");
        assert_eq!(d.cell_display_value(1, 1), ""); // number → empty
    }

    #[test]
    fn cell_function_address() {
        let mut d = DataProxy::new("MySheet");
        // CELL("address") without a reference defaults to the calling cell.
        d.set_cell_text(0, 0, "=CELL(\"address\")");
        assert_eq!(d.cell_display_value(0, 0), "A1");
    }

    #[test]
    fn cell_function_row_col() {
        let mut d = DataProxy::new("t");
        d.set_cell_text(5, 3, "=CELL(\"row\")");
        d.set_cell_text(5, 4, "=CELL(\"col\")");
        assert_eq!(d.cell_display_value(5, 3), "6"); // row 5 = 1-based row 6
        assert_eq!(d.cell_display_value(5, 4), "5"); // col 3 = 1-based col 5
    }

    #[test]
    fn cell_function_filename() {
        let mut d = DataProxy::new("Report");
        d.set_cell_text(0, 0, "=CELL(\"filename\")");
        assert_eq!(d.cell_display_value(0, 0), "Report");
    }

    #[test]
    fn hyperlink_returns_label() {
        let mut d = DataProxy::new("t");
        d.set_cell_text(0, 0, "=HYPERLINK(\"https://example.com\", \"Click\")");
        d.set_cell_text(0, 1, "=HYPERLINK(\"https://example.com\")");
        assert_eq!(d.cell_display_value(0, 0), "Click");
        assert_eq!(d.cell_display_value(0, 1), "https://example.com");
    }

    #[test]
    fn webservice_family_returns_value_error() {
        // WEBSERVICE / IMPORTXML / IMPORTHTML / IMPORTRANGE /
        // IMPORTDATA all need network access that's blocked in the
        // wasm sandbox. They're registered so the parser doesn't
        // emit #NAME?; the runtime surfaces #VALUE! to signal
        // "not available in this build" (BACKLOG §1). A host
        // that needs them can implement them in JS via on_change.
        for name in ["WEBSERVICE", "IMPORTXML", "IMPORTHTML", "IMPORTDATA"] {
            let mut d = DataProxy::new("t");
            d.set_cell_text(0, 0, &format!("={name}(\"https://example.com\")"));
            // The cell shows the error literal, not #NAME?.
            let s = d.cell_display_value(0, 0);
            assert!(s.contains("#VALUE") || s == "#VALUE!", "{name} -> {s}");
        }
    }

    #[test]
    fn formula_var_p_evaluates() {
        let mut d = DataProxy::new("t");
        d.set_cell_text(0, 0, "2"); // A1
        d.set_cell_text(0, 1, "4"); // B1
        d.set_cell_text(0, 2, "6"); // C1
        d.set_cell_text(1, 0, "=VAR.P(A1:C1)");
        let v = d.cell_display_value(1, 0);
        assert!(
            v != "#NAME?" && v != "#VALUE!" && v != "0",
            "unexpected: {}",
            v
        );
        let n: f64 = v.parse().unwrap();
        assert!(n > 1.0 && n < 4.0, "VAR.P = {}", n);
    }

    // --- Phase 3.1: array literals ---
    //
    // The public `eval` helper returns the *displayed* value, which
    // collapses an array to its top-left. Tests that need the full
    // array shape drop down to the `eval_array` helper below, which
    // uses the same plumbing as the live render path but exposes
    // the underlying `Value`.

    fn eval_array(formula: &str, cells: &[(usize, usize)]) -> Value {
        let mut d = DataProxy::new("t");
        for &(r, c) in cells {
            d.set_cell_text(r, c, &(r + c).to_string());
        }
        d.set_cell_text(50, 50, formula);
        // `cell_display_value` round-trips through Value::Array → a
        // stringified top-left for our top-level expectations, but
        // we also want the full Value. Use the public formula entry
        // point: `set_cell_text` evaluates on commit when the cell
        // is part of the renderer state. We side-step that and
        // re-evaluate via a workaround — read the rendered string
        // AND assert via `cell_display_value`, which is what users
        // see anyway. (The unit tests below check behaviour the
        // public display contract is sufficient for.)
        let _ = d.cell_display_value(50, 50);
        // For deeper structure assertions we still need Value.
        // The cleanest hook is to compile and call via the
        // renderer's evaluator path — but since that requires a
        // TableRenderer, the rest of this section asserts on the
        // *displayed* string plus error-free behaviour, which is
        // what callers actually observe.
        Value::Blank
    }

    #[test]
    fn array_literal_inside_sum() {
        // SUM({1,2,3,4}) = 10. The literal spills into an array,
        // SUM collapses it. The displayed value is "10".
        assert_eq!(eval("=SUM({1,2,3,4})", &[]), "10");
    }

    #[test]
    fn array_literal_broadcasts_with_scalar() {
        // {1,2,3}+10 broadcasts to {11,12,13}; displayed top-left
        // is 11. No error means the engine accepted the literal.
        assert_eq!(eval("={1,2,3}+10", &[]), "11");
    }

    #[test]
    fn array_literal_nested_in_arithmetic() {
        // {1,2;3,4} + {10,20;30,40} → top-left = 11.
        assert_eq!(eval("={1,2;3,4}+{10,20;30,40}", &[]), "11");
    }

    #[test]
    fn array_literal_top_left_display() {
        // The cell holding the literal collapses to its top-left.
        assert_eq!(eval("={42,99}", &[]), "42");
        assert_eq!(eval("={1;2;3}", &[]), "1");
    }

    #[test]
    fn array_literal_invalid_shape_returns_value_error() {
        // Mixed-width rows → #VALUE!.
        assert_eq!(eval("={1,2;3}", &[]), "#VALUE!");
        // Empty literal → #VALUE!.
        assert_eq!(eval("={}", &[]), "#VALUE!");
    }

    #[test]
    fn array_literal_does_not_break_existing_evaluator() {
        // Sanity: a plain arithmetic formula on the same engine
        // still works alongside the new array-literal path.
        assert_eq!(eval("=2+3", &[]), "5");
        assert_eq!(eval("=SUM({1;2;3})", &[]), "6");
    }

    // --- Phase 3.2: LET ---

    #[test]
    fn let_simple_binding() {
        // =LET(x, 5, x*2) → 10
        assert_eq!(eval("=LET(x,5,x*2)", &[]), "10");
    }

    #[test]
    fn let_repeated_name_uses_latest_value() {
        // LET(x, 1, x + LET(x, 2, x)) → 1 + 2 = 3.
        // The inner LET shadows x within its body; the outer
        // binding is unaffected by the inner redefinition.
        assert_eq!(eval("=LET(x,1,x+LET(x,2,x))", &[]), "3");
    }

    #[test]
    fn let_binding_does_not_leak_after_body() {
        // After the body returns, the binding is gone: a
        // subsequent reference to `x` is #NAME?.
        let sheet = "=LET(x,7,x)+1+IF(TRUE(),0,x)";
        // The outer `x` is undefined → #NAME? propagates. Use
        // 0+0 instead of 0+x to keep the test deterministic
        // without parsing the failure surface twice.
        let _ = sheet;
        // Direct test: a separate LET call after a referencing
        // expression must NOT see the prior binding.
        let mut d = DataProxy::new("t");
        d.set_cell_text(0, 0, "=LET(x,5,x)");
        d.set_cell_text(1, 0, "=x");
        // After commit the formula in (0,0) returns 5; the
        // formula in (1,0) references an undefined name → #NAME?.
        assert_eq!(d.cell_display_value(0, 0), "5");
        assert_eq!(d.cell_display_value(1, 0), "#NAME?");
    }

    #[test]
    fn let_multiple_bindings() {
        // =LET(a, 2, b, 3, a*b + a + b) → 6 + 2 + 3 = 11.
        assert_eq!(eval("=LET(a,2,b,3,a*b+a+b)", &[]), "11");
    }

    #[test]
    fn let_odd_arg_count_is_value_error() {
        // LET requires name, value, …, body — only 1 pair (no body)
        // is #VALUE!.
        assert_eq!(eval("=LET(x,1)", &[]), "#VALUE!");
        // 2 pairs + body → let body evaluate the undefined name.
        assert_eq!(eval("=LET(x,1,y)", &[]), "#NAME?");
    }

    #[test]
    fn let_three_args_minimum() {
        // LET(name, value, body) is the minimum valid shape.
        assert_eq!(eval("=LET(x,42,x)", &[]), "42");
    }

    #[test]
    fn let_value_can_reference_previous_binding() {
        // LET(a, 1, b, a+10, a+b) → b = 11, body = 12.
        assert_eq!(eval("=LET(a,1,b,a+10,a+b)", &[]), "12");
    }

    #[test]
    fn let_undefined_name_in_body_is_name_error() {
        // LET(x, 5, y*2) → #NAME? (y is not bound).
        assert_eq!(eval("=LET(x,5,y*2)", &[]), "#NAME?");
    }

    // --- Phase 3.3: LAMBDA + MAP ---

    #[test]
    fn lambda_defines_a_value() {
        // LAMBDA on its own produces a value (not a #VALUE!) — the
        // display collapses to "[LAMBDA]".
        assert_eq!(eval("=LAMBDA(x,x*2)", &[]), "[LAMBDA]");
    }

    #[test]
    fn map_applies_lambda_to_each_element() {
        // MAP({1,2,3}, LAMBDA(x, x*2)) → {2,4,6}.
        assert_eq!(eval("=MAP({1,2,3},LAMBDA(x,x*2))", &[]), "2");
    }

    #[test]
    fn map_with_multi_arg_lambda_returns_value_error() {
        // The lambda must take exactly one argument for MAP; the
        // inner body is short-circuited and the call returns
        // #VALUE! before invoking the body.
        assert_eq!(eval("=MAP({1,2,3},LAMBDA(x,y,x+y))", &[]), "#VALUE!");
    }

    #[test]
    fn map_with_non_lambda_second_arg_is_value_error() {
        // The second arg must be a LAMBDA — passing a number
        // can't be called.
        assert_eq!(eval("=MAP({1,2,3},5)", &[]), "#VALUE!");
    }

    #[test]
    fn map_wrong_arg_count_is_value_error() {
        assert_eq!(eval("=MAP({1,2,3})", &[]), "#VALUE!");
        assert_eq!(eval("=MAP(LAMBDA(x,x))", &[]), "#VALUE!");
    }

    #[test]
    fn lambda_with_let_captures_binding() {
        // LET-then-MAP: the let-binding flows into the lambda's
        // body, the lambda's parameter (`x`) is independent.
        // =LET(scale, 10, MAP({1,2,3}, LAMBDA(x, x*scale))) →
        // {10, 20, 30}.
        assert_eq!(
            eval("=LET(scale,10,MAP({1,2,3},LAMBDA(x,x*scale)))", &[]),
            "10"
        );
    }

    #[test]
    fn lambda_param_shadows_let_binding_inside_body() {
        // =LET(x, 99, MAP({1,2}, LAMBDA(x, x+1))) → {2, 3}.
        // The lambda's parameter `x` shadows the LET binding
        // inside the lambda's body; outside the lambda, the
        // outer x still exists in the LET frame, but MAP doesn't
        // reference it.
        assert_eq!(eval("=LET(x,99,MAP({1,2},LAMBDA(x,x+1)))", &[]), "2");
    }

    #[test]
    fn map_with_scalar_input_returns_1x1() {
        // =MAP(5, LAMBDA(x, x*10)) → {50}. Top-left = 50.
        assert_eq!(eval("=MAP(5,LAMBDA(x,x*10))", &[]), "50");
    }

    #[test]
    fn lambda_with_no_params_rejected_by_map() {
        // MAP requires the lambda to take exactly one argument; a
        // parameterless lambda is rejected before the body runs.
        assert_eq!(eval("=MAP({1,2,3},LAMBDA(42))", &[]), "#VALUE!");
    }

    #[test]
    fn lambda_definition_round_trip_through_let() {
        // Composing LET + LAMBDA: the lambda is stored as a
        // Value::Lambda inside the let frame, then MAP pulls it
        // out and applies it.
        let mut d = DataProxy::new("t");
        d.set_cell_text(0, 0, "=LET(double,LAMBDA(x,x*2),MAP({1,2,3,4},double))");
        assert_eq!(d.cell_display_value(0, 0), "2");
    }

    // --- Phase 3.4: REDUCE / BYROW / BYCOL / MAKEARRAY ---
    //
    // Each one reuses call_lambda so the LET-binding frame and
    // lambda-body evaluation are unchanged. The arguments
    // validated here are the **count** (2 for REDUCE/MAKEARRAY,
    // 1 for BYROW/BYCOL) and the basic shape of the result.

    #[test]
    fn reduce_sums_with_initial_zero() {
        // =REDUCE(0, {1,2,3,4}, LAMBDA(acc,x,acc+x)) → 10.
        assert_eq!(eval("=REDUCE(0,{1,2,3,4},LAMBDA(acc,x,acc+x))", &[]), "10");
    }

    #[test]
    fn reduce_with_non_zero_initial() {
        // Initial accumulator is respected.
        assert_eq!(
            eval("=REDUCE(100,{1,2,3,4},LAMBDA(acc,x,acc+x))", &[]),
            "110"
        );
    }

    #[test]
    fn reduce_wrong_lambda_arity_is_value_error() {
        // LAMBDA takes 1 param; REDUCE needs 2.
        assert_eq!(eval("=REDUCE(0,{1,2},LAMBDA(a,a+1))", &[]), "#VALUE!");
    }

    #[test]
    fn reduce_non_lambda_second_arg_is_value_error() {
        // The accumulator argument doesn't matter; the second
        // arg must still be a lambda.
        assert_eq!(eval("=REDUCE(0,{1,2},99)", &[]), "#VALUE!");
    }

    #[test]
    fn reduce_wrong_arg_count_is_value_error() {
        assert_eq!(eval("=REDUCE(0,{1,2})", &[]), "#VALUE!");
        assert_eq!(eval("=REDUCE(0)", &[]), "#VALUE!");
    }

    #[test]
    fn byrow_sums_each_row() {
        // 2×3 array {1,2,3;4,5,6}, lambda gets each row,
        // SUM reduces. Result: a column vector {6, 15}.
        assert_eq!(eval("=BYROW({1,2,3;4,5,6},LAMBDA(r,SUM(r)))", &[]), "6");
    }

    #[test]
    fn byrow_wrong_lambda_arity_is_value_error() {
        // Lambda must take 1 param (the row array).
        assert_eq!(eval("=BYROW({1,2;3,4},LAMBDA(a,b,a+b))", &[]), "#VALUE!");
    }

    #[test]
    fn bycol_passes_each_column() {
        // 2×3 array, lambda receives each column as its 1-D
        // argument. =BYCOL({1,2,3;4,5,6}, LAMBDA(c,SUM(c)))
        // returns {5, 7, 9} as a single column → top-left = 5.
        assert_eq!(eval("=BYCOL({1,2,3;4,5,6},LAMBDA(c,SUM(c)))", &[]), "5");
    }

    #[test]
    fn makearray_builds_grid() {
        // =MAKEARRAY(3,2, LAMBDA(i,j,i+j)) → {{2,3},{3,4},{4,5}};
        // top-left = 2.
        assert_eq!(eval("=MAKEARRAY(3,2,LAMBDA(i,j,i+j))", &[]), "2");
    }

    #[test]
    fn makearray_wrong_lambda_arity_is_value_error() {
        // Lambda must take 2 params (i, j).
        assert_eq!(eval("=MAKEARRAY(2,2,LAMBDA(k,k*2))", &[]), "#VALUE!");
    }

    #[test]
    fn reduce_borrow_row_col_makearray_pair_with_let() {
        // All four work inside a LET frame.
        let mut d = DataProxy::new("t");
        // REDUCE via LET to share a multiplier.
        d.set_cell_text(0, 0, "=LET(k,10,REDUCE(0,{1,2,3},LAMBDA(a,x,a+x*k)))");
        assert_eq!(d.cell_display_value(0, 0), "60");
        // BYROW via LET to scale each row's SUM.
        d.set_cell_text(
            1,
            0,
            "=LET(scale,2,BYROW({1,2;3,4},LAMBDA(r,SUM(r)*scale)))",
        );
        assert_eq!(d.cell_display_value(1, 0), "6");
        // MAKEARRAY via LET to parameterise a generated grid.
        d.set_cell_text(2, 0, "=LET(fill,9,MAKEARRAY(2,2,LAMBDA(i,j,i*10+j+fill)))");
        // i=1,j=1 → 10+1+9=20; i=1,j=2 → 10+2+9=21;
        // i=2,j=1 → 20+1+9=30; i=2,j=2 → 20+2+9=31.
        assert_eq!(d.cell_display_value(2, 0), "20");
    }

    // --- Page breaks (Phase 5.1) ---

    #[test]
    fn page_setup_default_has_empty_page_breaks() {
        // The default constructor must yield an empty list — no
        // breaks pre-seeded.
        assert!(PageSetup::default().page_breaks.is_empty());
    }

    #[test]
    fn page_break_serde_roundtrip() {
        // Set two breaks (one row, one col), serialize, deserialize,
        // and confirm both came back in the same shape.
        let mut ps = PageSetup::default();
        ps.page_breaks.push(PageBreak {
            row: Some(5),
            col: None,
        });
        ps.page_breaks.push(PageBreak {
            row: None,
            col: Some(3),
        });
        let json = serde_json::to_string(&ps).unwrap();
        assert!(json.contains("page_breaks"));
        let back: PageSetup = serde_json::from_str(&json).unwrap();
        assert_eq!(back.page_breaks.len(), 2);
        assert_eq!(back.page_breaks[0].row, Some(5));
        assert_eq!(back.page_breaks[1].col, Some(3));
    }

    #[test]
    fn page_setup_pre_5_1_workbook_loads_with_empty_breaks() {
        // A pre-1.5 workbook JSON that lacks `page_breaks` must still
        // load — `#[serde(default)]` fills the field with an empty
        // Vec. The skip_serializing_if on the empty case keeps
        // backward-compat round-trips identical.
        let legacy = r##"{
            "orientation": "portrait",
            "paper_size": "letter",
            "margins": [0.75, 0.75, 0.75, 0.75],
            "scale": 100
        }"##;
        let ps: PageSetup = serde_json::from_str(legacy).unwrap();
        assert!(ps.page_breaks.is_empty());
        // Round-trip emits no `page_breaks` key.
        let back = serde_json::to_string(&ps).unwrap();
        assert!(!back.contains("page_breaks"));
    }
}
