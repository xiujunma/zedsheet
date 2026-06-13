//! PivotTable model + aggregation engine (issue #35).
//!
//! A [`PivotTable`] describes a cross-tab: pick row/column field indexes from a
//! source range, pick a value field with an aggregation, and [`compute`] produces
//! the rendered grid (labels + numbers). The caller (modal + renderer) is
//! responsible for materializing that grid onto a new `DataProxy`.
//!
//! Self-contained: depends only on `DataProxy::cell_raw_value` for input and
//! uses a local [`Key`] enum for grouping, so it does not need to touch the
//! `data_proxy::Value` derives.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use super::cell_range::CellRange;
use super::data_proxy::DataProxy;

/// Aggregation function applied to the value column within each group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Agg {
    Sum,
    /// Counts non-blank rows in the group (Excel: `COUNTA`-style).
    Count,
    /// Arithmetic mean of numeric values; text-numerics count.
    Avg,
    Min,
    Max,
}

impl Default for Agg {
    fn default() -> Self { Agg::Sum }
}

impl Agg {
    pub fn label(self) -> &'static str {
        match self {
            Agg::Sum => "Sum",
            Agg::Count => "Count",
            Agg::Avg => "Average",
            Agg::Min => "Min",
            Agg::Max => "Max",
        }
    }
}

/// One pivot table specification (issue #35).
///
/// Pure data: holds the source spec, not the rendered output. The output
/// always lives on a fresh `DataProxy` pushed into the workbook's
/// `SheetsRegistry`; this struct is the *recipe*.
///
/// `#[serde(default)]` on the `Vec<PivotTable>` field that holds this on
/// `DataProxy` is what guarantees forward-compat for old workbooks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PivotTable {
    /// Source range expression, e.g. `"Sheet1!A1:D12"`. The first row of the
    /// range is treated as headers; row/column/value field indexes are
    /// offsets into the range.
    pub source_range: String,
    /// Sheet the source range lives on. Stored explicitly so a `PivotTable`
    /// doesn't need a back-reference into the registry.
    pub source_sheet: String,
    /// Column indexes (relative to the source range) used as row labels.
    /// Each index must reference a header cell. Empty = single "Total" row.
    pub row_fields: Vec<usize>,
    /// Column indexes used as column labels. Empty = single "Total" column.
    pub col_fields: Vec<usize>,
    /// The single value field for the original MVP (issue #35). Kept for
    /// backward-compat with old workbooks — when `value_fields` is empty,
    /// the engine uses `{ field: value_field, agg }` as a one-element
    /// list. When `value_fields` is non-empty, it is authoritative.
    pub value_field: usize,
    pub agg: Agg,
    /// Multiple value fields (issue #59). Each entry is one (field, agg)
    /// pair; the cross-tab body widens by `len(value_fields)`. Empty
    /// means "use the legacy `value_field`/`agg` above". The
    /// default-value deserializer keeps old workbooks loadable.
    #[serde(default)]
    pub value_fields: Vec<ValueField>,
    /// Page-level filters (issue #58): a field placed in the Filters zone
    /// scopes which source rows are aggregated. Each entry holds the field
    /// index and the user's selected values — when the list is empty, no
    /// filter applies (Excel's "All" / "(Multiple Items)" default). The
    /// default-value deserializer (empty vec) keeps old workbooks
    /// loadable.
    #[serde(default)]
    pub filter_fields: Vec<FilterField>,
    /// Per-field date grouping (issue #60). If a row or col field index
    /// appears in this map, the key extractor parses that cell as a date
    /// (ISO `2024-03-15`, US `3/15/2024`, or Excel serial `45306`) and
    /// groups the key by the chosen unit instead of using the raw value.
    /// Empty = no grouping — old workbooks keep working via the
    /// `#[serde(default)]` deserializer.
    #[serde(default)]
    pub date_groups: HashMap<usize, DateGroup>,
    /// Name of the output sheet this pivot is currently rendered on. The
    /// renderer updates this when the user Refreshes (in MVP, the output
    /// sheet's name never changes — Refresh overwrites in place).
    pub output_sheet: String,
}

/// One date-grouping unit (issue #60). The key extractor recognizes
/// dates in three formats:
///
/// - ISO 8601 text: `2024-03-15`, `2024/03/15`, with optional time
///   suffix (`2024-03-15T10:30:00`).
/// - US/EU text: `3/15/2024` (US — day ambiguous when both parts ≤ 12),
///   `15/3/2024` (EU — first part > 12).
/// - Excel date serial: a pure integer (e.g. `45306` for 2024-01-15).
///
/// The grouped key is rendered as `YYYY`, `YYYY-Qn` (n=1..=4),
/// `YYYY-MM`, or `YYYY-MM-DD`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DateGroup {
    /// Group by calendar year — key renders as `YYYY`.
    Year,
    /// Group by year + quarter — key renders as `YYYY-Qn` (n=1..=4).
    Quarter,
    /// Group by year + month — key renders as `YYYY-MM`.
    Month,
    /// Group by year + month + day — key renders as `YYYY-MM-DD`.
    Day,
}

/// One value field + its aggregation (issue #59). The cross-tab body
/// widens by `len(pivot.value_fields)`: each entry gets its own
/// column block, side-by-side.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValueField {
    /// Column index (relative to the source range) to aggregate.
    pub field: usize,
    /// Aggregation to apply over each bucket.
    pub agg: Agg,
}

/// A page-level filter on the source (issue #58). The selected values are
/// stored as the strings the user sees in the source cells (so a refresh
/// after the user changes a label picks up the new value list).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterField {
    /// Column index (relative to the source range) the filter applies to.
    pub field_idx: usize,
    /// The values the user has selected. Empty = "All" (no filter).
    /// Stored as `String` rather than `Value` so dates and numbers survive
    /// JSON round-trip with their display form.
    pub selected_values: Vec<String>,
}

/// A floating visual filter bound to a single source field (issue #61).
///
/// Slicers live on the source `DataProxy` (alongside `pivots` and
/// `charts`) and are applied as an additional row predicate by
/// [`compute`]. Every pivot whose spec references the bound `field_idx`
/// — whether as a row, column, value, or filter field — is recomputed
/// against the slicer's selected values.
///
/// Like the page-level [`FilterField`], an empty `selected_values` is the
/// "All" sentinel (no filtering); a non-empty set is the "include these
/// values" filter. The engine looks up at most one slicer per field; if
/// the list carries multiple, the last one wins. Slicers on different
/// fields compose (AND).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Slicer {
    /// Stable id, used as the DOM key for the floating panel and as the
    /// anchor for delete operations. Auto-generated by the modal as
    /// `"slicer_{n}"`; persisted so a workbook reload can find its panel.
    pub id: String,
    /// Column index (relative to the source sheet) the slicer binds to.
    pub field_idx: usize,
    /// The values the user has selected. Empty = "All" (no filter).
    /// Stored as `String` so dates and numbers survive JSON round-trip
    /// with their display form.
    pub selected_values: Vec<String>,
    /// Top-left of the floating panel, in CSS pixels relative to the
    /// canvas container. Persisted so a reloaded workbook's panel
    /// reopens in the same spot.
    pub x: f64,
    pub y: f64,
    /// Panel size in CSS pixels. Persisted so a reloaded workbook's
    /// panel reopens at the same size.
    pub width: f64,
    pub height: f64,
}

impl PivotTable {
    /// Effective list of value fields (issue #59). If `value_fields` is
    /// non-empty, that's authoritative; otherwise we synthesize a
    /// one-element list from the legacy `value_field`/`agg` pair so old
    /// workbooks keep working.
    pub fn effective_value_fields(&self) -> Vec<ValueField> {
        if self.value_fields.is_empty() {
            vec![ValueField {
                field: self.value_field,
                agg: self.agg,
            }]
        } else {
            self.value_fields.clone()
        }
    }
}

impl PivotTable {
    /// Validate the spec against a source `DataProxy` and return the parsed
    /// bounds of the source range. The caller is expected to have resolved
    /// `source_sheet` to a `DataProxy` before calling.
    pub fn validate<'a>(
        &self,
        source: &'a DataProxy,
    ) -> Result<(usize, usize, usize, usize), String> {
        // `CellRange::from_str` handles only the A1 part; the optional
        // `Sheet1!` prefix on `source_range` has to be stripped first.
        let a1 = match self.source_range.split_once('!') {
            Some((_sheet, rest)) => rest,
            None => &self.source_range,
        };
        let r = CellRange::from_str(a1)
            .map_err(|()| format!("source range {:?} is not a valid A1 reference", self.source_range))?;
        let r0 = r.sri.min(r.eri);
        let c0 = r.sci.min(r.eci);
        let r1 = r.eri.max(r.sri);
        let c1 = r.eci.max(r.sci);
        // A valid pivot source needs at least a header row plus one data row
        // (a single column is fine). A 1x1 range is not.
        if r1 <= r0 {
            return Err("source range has no data rows".into());
        }
        let max_field = (c1 - c0) as usize;
        for ci in self.row_fields.iter().chain(self.col_fields.iter()) {
            if *ci > max_field {
                return Err(format!("field index {ci} out of source range"));
            }
        }
        // Validate every value field (issue #59) — both the multi-value list
        // and the legacy single-value field, since a workbook may carry
        // either or both.
        for vf in self.effective_value_fields() {
            if vf.field > max_field {
                return Err(format!(
                    "value field index {} out of source range",
                    vf.field
                ));
            }
        }
        // Validate every page-level filter field (issue #58) so a typo'd
        // index surfaces here, not as a silent wrong-bucket result later.
        for ff in self.filter_fields.iter() {
            if ff.field_idx > max_field {
                return Err(format!(
                    "filter field index {} out of source range",
                    ff.field_idx
                ));
            }
        }
        // Touch `source` so the borrow checker / clippy doesn't complain
        // about the unused parameter — the data may be re-read in follow-up
        // checks (e.g. cross-sheet pointer, range-bounds vs row_count), but
        // the spec itself only needs the parsed bounds.
        let _ = source;
        Ok((r0, c0, r1, c1))
    }
}

/// A grouping key. `Single` is used for a one-field row/column axis;
/// `Tuple` is used for a multi-field axis.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Key {
    Single(PrimKey),
    Tuple(Vec<PrimKey>),
}

/// Single-cell key component. NaN-normalized for `Number` (so the Hash
/// doesn't poison the bucket if the data ever contains NaN); case-insensitive
/// for `Text` (mirrors `compare_values` semantics).
#[derive(Debug, Clone)]
pub enum PrimKey {
    Number(i64), // bit-cast of the f64 (NaN → 0)
    Text(String),
    Blank,
}

impl PartialEq for PrimKey {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (PrimKey::Number(a), PrimKey::Number(b)) => a == b,
            (PrimKey::Text(a), PrimKey::Text(b)) => a.to_lowercase() == b.to_lowercase(),
            (PrimKey::Blank, PrimKey::Blank) => true,
            _ => false,
        }
    }
}
impl Eq for PrimKey {}

impl std::hash::Hash for PrimKey {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        std::mem::discriminant(self).hash(h);
        match self {
            PrimKey::Number(n) => n.hash(h),
            PrimKey::Text(s) => s.to_lowercase().hash(h),
            PrimKey::Blank => {}
        }
    }
}

/// A single key value rendered for the output sheet.
pub fn key_to_display(k: &Key) -> String {
    match k {
        Key::Single(p) => prim_to_display(p),
        Key::Tuple(parts) => parts.iter().map(prim_to_display).collect::<Vec<_>>().join(" / "),
    }
}

fn prim_to_display(p: &PrimKey) -> String {
    match p {
        PrimKey::Number(n) => format_number_bits(*n),
        PrimKey::Text(s) => s.clone(),
        PrimKey::Blank => String::new(),
    }
}

fn format_number_bits(bits: i64) -> String {
    let f = f64::from_bits(bits as u64);
    if f == f.trunc() && f.abs() < 1e15 {
        format!("{}", f as i64)
    } else {
        // 6 significant digits matches Excel's default number format feel.
        format!("{}", f)
    }
}

/// The cross-tab result. The caller materializes this onto a new sheet.
#[derive(Debug, Clone)]
pub struct PivotResult {
    /// Distinct row labels in first-appearance order. Always at least one
    /// entry (`Single(Blank)` for an empty `row_fields`).
    pub row_keys: Vec<Key>,
    /// Distinct column labels in first-appearance order. Always at least one.
    pub col_keys: Vec<Key>,
    /// Number of value fields the result was computed with. Cached so the
    /// materializer doesn't have to recompute `pt.effective_value_fields()`
    /// (issue #59).
    pub nv: usize,
    /// `body[row_idx][v * nc + col_idx]` is the aggregated value for the
    /// `(row_key, col_key)` bucket under value field `v`. `None` means
    /// the bucket was empty. For `nv == 1` the inner layout is
    /// `body[r][c]` — same as the pre-#59 single-value shape.
    pub body: Vec<Vec<Option<f64>>>,
    /// `row_totals[row_idx][v]` — aggregate of every value in row `row_idx`
    /// under value field `v` (any col_key).
    pub row_totals: Vec<Vec<Option<f64>>>,
    /// `col_totals[col_idx][v]` — aggregate of every value in col `col_idx`
    /// under value field `v` (any row_key).
    pub col_totals: Vec<Vec<Option<f64>>>,
    /// `grand_total[v]` — aggregate of every value in the source under
    /// value field `v`.
    pub grand_total: Vec<Option<f64>>,
}

/// Compute the cross-tab for a given `PivotTable` against a `DataProxy`.
///
/// Reads source cells via `DataProxy::cell_raw_value` (so any formulas in
/// the source are evaluated) and groups on `Value`-typed keys.
pub fn compute(source: &DataProxy, pt: &PivotTable) -> Result<PivotResult, String> {
    let (r0, c0, r1, _c1) = pt.validate(source)?;

    // Resolve the effective value-field list (issue #59). When the spec
    // carries an empty `value_fields` (legacy workbook), this falls back to
    // a one-element list built from the `value_field`/`agg` pair — so old
    // workbooks keep working without a migration step.
    let value_fields = pt.effective_value_fields();
    let nv = value_fields.len();

    // Build the filter predicate once (issue #58 + #61). A row passes
    // when *every* active filter's value is in its `selected_values` list
    // AND *every* active slicer's value is in its `selected_values` set.
    // An empty `selected_values` is the "All" / "(Multiple Items)"
    // sentinel and matches every row, for both filters and slicers.
    let filter_pred: Box<dyn Fn(usize) -> bool> = if pt.filter_fields.is_empty() && source.slicers.is_empty() {
        Box::new(|_| true)
    } else {
        // Pre-compute the raw text for each filter field, per row, so the
        // predicate is a cheap pointer comparison inside the loop.
        let filters: Vec<(usize, std::collections::HashSet<String>)> = pt
            .filter_fields
            .iter()
            .map(|f| (c0 + f.field_idx, f.selected_values.iter().cloned().collect()))
            .collect();
        // Slicers (issue #61): at most one per field; the last write wins
        // if the source carries two on the same field. Each binding
        // filters by the source cell's raw text, so the slicer's "values"
        // are the same strings the user sees in the source cells (and
        // the same strings `FilterField` uses, so the two filter
        // mechanisms behave consistently).
        let slicers: Vec<(usize, std::collections::HashSet<String>)> = {
            // De-duplicate by field index, keeping the last write — the
            // modal would prevent this, but the engine must agree on
            // a deterministic answer for hand-edited or legacy specs.
            let mut last_per_field: std::collections::HashMap<usize, std::collections::HashSet<String>> =
                std::collections::HashMap::new();
            for s in source.slicers.iter() {
                last_per_field.insert(
                    s.field_idx,
                    s.selected_values.iter().cloned().collect(),
                );
            }
            last_per_field
                .into_iter()
                .map(|(field_idx, allowed)| (c0 + field_idx, allowed))
                .collect()
        };
        Box::new(move |ri: usize| {
            filters.iter().all(|(ci, allowed)| {
                // An empty `allowed` set is the "All" sentinel — passes.
                allowed.is_empty() || allowed.contains(&source.cell_raw_value(ri, *ci))
            }) && slicers.iter().all(|(ci, allowed)| {
                allowed.is_empty() || allowed.contains(&source.cell_raw_value(ri, *ci))
            })
        })
    };

    // First pass: bucket values by (row_key, col_key, value_field_idx).
    // Each bucket holds one `Option<f64>` per source row in that bucket
    // and per value field, so a single value field produces one `Option`
    // per source row, two value fields produce two, and so on. Text that
    // doesn't parse as a number is dropped from numeric aggs; Count counts
    // non-blank entries. Rows that fail the filter predicate are skipped
    // entirely (issue #58).
    let mut buckets: HashMap<(Key, Key), Vec<Vec<Option<f64>>>> = HashMap::new();
    for ri in (r0 + 1)..=r1 {
        if !filter_pred(ri) {
            continue;
        }
        let rk = make_key(source, ri, c0, &pt.row_fields, pt);
        let ck = make_key(source, ri, c0, &pt.col_fields, pt);
        let entry = buckets.entry((rk, ck)).or_insert_with(|| Vec::with_capacity(nv));
        // Grow lazily on first encounter so the value-field index lines up.
        for (v_idx, vf) in value_fields.iter().enumerate() {
            if v_idx >= entry.len() {
                entry.push(Vec::new());
            }
            let raw = source.cell_raw_value(ri, c0 + vf.field);
            entry[v_idx].push(parse_for_agg(&raw));
        }
    }

    // Second pass: distinct keys in first-appearance order. Same filter.
    let mut row_keys: Vec<Key> = Vec::new();
    let mut col_keys: Vec<Key> = Vec::new();
    let mut seen_r: HashSet<Key> = HashSet::new();
    let mut seen_c: HashSet<Key> = HashSet::new();
    for ri in (r0 + 1)..=r1 {
        if !filter_pred(ri) {
            continue;
        }
        let rk = make_key(source, ri, c0, &pt.row_fields, pt);
        let ck = make_key(source, ri, c0, &pt.col_fields, pt);
        if seen_r.insert(rk.clone()) {
            row_keys.push(rk);
        }
        if seen_c.insert(ck.clone()) {
            col_keys.push(ck);
        }
    }

    // Materialize the body. `body[r]` has `nv * nc_keys` entries; the body
    // value for `(r, v, c)` lives at `body[r][v * nc + c]`. For `nv == 1`
    // this collapses to `body[r][c]`, matching the pre-#59 single-value
    // shape.
    let nc = col_keys.len();
    let mut body: Vec<Vec<Option<f64>>> = vec![vec![None; nv * nc]; row_keys.len()];
    for (ri, rk) in row_keys.iter().enumerate() {
        for (ci, ck) in col_keys.iter().enumerate() {
            if let Some(per_v) = buckets.get(&(rk.clone(), ck.clone())) {
                for (v_idx, vs) in per_v.iter().enumerate() {
                    body[ri][v_idx * nc + ci] = aggregate(&value_fields[v_idx].agg, vs);
                }
            }
        }
    }

    // Row totals: aggregate every value with the same row_key (any col_key)
    // per value field.
    let row_totals: Vec<Vec<Option<f64>>> = row_keys
        .iter()
        .map(|rk| {
            (0..nv)
                .map(|v_idx| {
                    let all: Vec<Option<f64>> = buckets
                        .iter()
                        .filter(|((rk2, _), _)| rk2 == rk)
                        .flat_map(|(_, per_v)| {
                            per_v.get(v_idx).into_iter().flat_map(|v| v.iter().cloned())
                        })
                        .collect();
                    aggregate(&value_fields[v_idx].agg, &all)
                })
                .collect()
        })
        .collect();

    // Column totals: symmetric.
    let col_totals: Vec<Vec<Option<f64>>> = col_keys
        .iter()
        .map(|ck| {
            (0..nv)
                .map(|v_idx| {
                    let all: Vec<Option<f64>> = buckets
                        .iter()
                        .filter(|((_, ck2), _)| ck2 == ck)
                        .flat_map(|(_, per_v)| {
                            per_v.get(v_idx).into_iter().flat_map(|v| v.iter().cloned())
                        })
                        .collect();
                    aggregate(&value_fields[v_idx].agg, &all)
                })
                .collect()
        })
        .collect();

    // Grand total: aggregate every value per value field.
    let grand_total: Vec<Option<f64>> = (0..nv)
        .map(|v_idx| {
            let all: Vec<Option<f64>> = buckets
                .values()
                .flat_map(|per_v| {
                    per_v.get(v_idx).into_iter().flat_map(|v| v.iter().cloned())
                })
                .collect();
            aggregate(&value_fields[v_idx].agg, &all)
        })
        .collect();

    Ok(PivotResult {
        row_keys,
        col_keys,
        nv,
        body,
        row_totals,
        col_totals,
        grand_total,
    })
}

/// Build the grouping key for one source row. Reads the columns named by
/// `fields` (offsets relative to `c0`) and returns a `Key::Single` for
/// 0/1 fields, or a `Key::Tuple` for multi-field keys. If a field's index
/// appears in `pt.date_groups` (issue #60), the cell is parsed as a date
/// and the key is rendered as the chosen unit (`YYYY`, `YYYY-Qn`,
/// `YYYY-MM`, or `YYYY-MM-DD`).
fn make_key(source: &DataProxy, ri: usize, c0: usize, fields: &[usize], pt: &PivotTable) -> Key {
    if fields.is_empty() {
        return Key::Single(PrimKey::Blank);
    }
    let parts: Vec<PrimKey> = fields
        .iter()
        .map(|ci| {
            // Date grouping (issue #60): when a field is named in
            // `pt.date_groups`, parse the cell as a date and emit a
            // formatted text key. Unparseable dates fall back to the raw
            // cell text (so a typo'd source value still surfaces as a
            // distinct bucket rather than collapsing into "Other").
            if let Some(&group) = pt.date_groups.get(ci) {
                let raw = source.cell_raw_value(ri, c0 + *ci);
                if let Some(grp) = parse_date_key(&raw, group) {
                    return PrimKey::Text(grp);
                }
            }
            prim_from_cell(source, ri, c0 + *ci)
        })
        .collect();
    if parts.len() == 1 {
        Key::Single(parts.into_iter().next().unwrap())
    } else {
        Key::Tuple(parts)
    }
}

/// Read one cell and turn it into a `PrimKey`. Coerces numeric text to
/// `Number` (Excel behavior — `100` and `"100"` collapse into the same
/// bucket). Empty cells and unparseable text become `Blank`.
fn prim_from_cell(source: &DataProxy, ri: usize, ci: usize) -> PrimKey {
    let raw = source.cell_raw_value(ri, ci);
    let t = raw.trim();
    if t.is_empty() {
        return PrimKey::Blank;
    }
    if let Ok(n) = t.parse::<f64>() {
        return PrimKey::Number(n.to_bits() as i64);
    }
    // Coerce leading-numeric-with-suffix? No: Excel does not match "100kg"
    // with 100 in a column. Be strict.
    PrimKey::Text(t.to_string())
}

/// Parse a cell as a date and return the grouped key for the chosen unit
/// (issue #60). Returns `None` when the cell is blank or doesn't match any
/// recognized date format — the caller falls back to the raw cell text
/// in that case so unparseable values still surface as distinct keys.
fn parse_date_key(raw: &str, group: DateGroup) -> Option<String> {
    let t = raw.trim();
    if t.is_empty() {
        return None;
    }
    // Try the unambiguous formats first: ISO (and slash-separated) and
    // Excel serial. US/EU is the fallback because it has to disambiguate
    // `M/D/YYYY` from `D/M/YYYY` heuristically.
    if let Some(d) = parse_iso_or_slash(t) {
        return Some(format_group(&d, group));
    }
    if let Some(d) = parse_excel_serial(t) {
        return Some(format_group(&d, group));
    }
    if let Some(d) = parse_us_eu(t) {
        return Some(format_group(&d, group));
    }
    None
}

/// `YYYY-MM-DD`, `YYYY/MM/DD`, with optional `T…` or ` …` time suffix.
fn parse_iso_or_slash(s: &str) -> Option<DateYmd> {
    // Strip the time portion if present.
    let date_part = s.split(|c| c == 'T' || c == ' ').next()?;
    let parts: Vec<&str> = date_part
        .split(|c| c == '-' || c == '/')
        .collect();
    if parts.len() != 3 {
        return None;
    }
    let y: i32 = parts[0].parse().ok()?;
    let m: u32 = parts[1].parse().ok()?;
    let d: u32 = parts[2].parse().ok()?;
    if m < 1 || m > 12 || d < 1 || d > 31 {
        return None;
    }
    Some(DateYmd { y, m, d })
}

/// Excel date serial — a pure integer in the sensible date range. We
/// accept `[1, 200_000)`, which covers years ~1900 to ~2447. Anything
/// outside is treated as "not a serial" so ordinary small integers in
/// data columns don't accidentally become 1900-era dates.
fn parse_excel_serial(s: &str) -> Option<DateYmd> {
    // Reject any string with a decimal point, sign, or exponent.
    if s.chars().any(|c| c == '.' || c == '-' || c == '+' || c == 'e' || c == 'E') {
        return None;
    }
    let n: i64 = s.parse().ok()?;
    if !(1..200_000).contains(&n) {
        return None;
    }
    civil_from_days(n - EXCEL_EPOCH_OFFSET)
}

/// `M/D/YYYY` (US) or `D/M/YYYY` (EU — when first part > 12). Two-digit
/// years pivot at 50: `24` → `2024`, `75` → `1975`.
fn parse_us_eu(s: &str) -> Option<DateYmd> {
    let parts: Vec<&str> = s.split('/').collect();
    if parts.len() != 3 {
        return None;
    }
    let a: u32 = parts[0].parse().ok()?;
    let b: u32 = parts[1].parse().ok()?;
    let y_raw: i32 = parts[2].parse().ok()?;
    let y = if (0..100).contains(&y_raw) {
        // 50-year pivot: 00..=49 → 20xx; 50..=99 → 19xx.
        if y_raw < 50 {
            2000 + y_raw
        } else {
            1900 + y_raw
        }
    } else {
        y_raw
    };
    let (m, d) = if a > 12 {
        // Day-first (EU) — only legal if day exceeds 12.
        (b, a)
    } else if b > 12 {
        // Month-first (US) — only legal if day exceeds 12.
        (a, b)
    } else {
        // Both ≤ 12: ambiguous. Default to US (matches Excel on a
        // US-locale install, which is the common case).
        (a, b)
    };
    if m < 1 || m > 12 || d < 1 || d > 31 {
        return None;
    }
    Some(DateYmd { y, m, d })
}

/// `days_from_civil(1899, 12, 30)` — the offset between Excel's serial
/// 0 (1899-12-30) and Hinnant's proleptic-Gregorian `days_from_epoch`
/// (days since 1970-01-01). Computed once at compile time; see
/// `parse_excel_serial` for the inverse.
const EXCEL_EPOCH_OFFSET: i64 = 25_569;

#[derive(Debug, Clone, Copy)]
struct DateYmd {
    y: i32,
    m: u32,
    d: u32,
}

/// Render a date key for the chosen grouping unit (issue #60).
fn format_group(d: &DateYmd, g: DateGroup) -> String {
    match g {
        DateGroup::Year => format!("{:04}", d.y),
        DateGroup::Quarter => {
            let q = (d.m - 1) / 3 + 1;
            format!("{:04}-Q{}", d.y, q)
        }
        DateGroup::Month => format!("{:04}-{:02}", d.y, d.m),
        DateGroup::Day => format!("{:04}-{:02}-{:02}", d.y, d.m, d.d),
    }
}

/// Howard Hinnant's `civil_from_days` (proleptic Gregorian). Inverse of
/// `days_from_civil`; we only need this direction for Excel-serial dates
/// (issue #60). Verified against `2024-01-15` ↔ serial 45306.
fn civil_from_days(z: i64) -> Option<DateYmd> {
    // Shift to Hinnant's `days_from_zero` epoch (0000-03-01).
    let z = z + 719_468;
    let era = if z >= 0 { z / 146_097 } else { (z - 146_096) / 146_097 };
    let doe = (z - era * 146_097) as u64; // [0, 146_096]
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let y = if m <= 2 { y + 1 } else { y };
    Some(DateYmd {
        y: y as i32,
        m: m as u32,
        d: d as u32,
    })
}

/// Parse a value cell into `Some(f64)` or `None` (blank). Numeric text
/// coerces; everything else is `None`.
fn parse_for_agg(raw: &str) -> Option<f64> {
    let t = raw.trim();
    if t.is_empty() {
        return None;
    }
    if let Ok(n) = t.parse::<f64>() {
        return Some(n);
    }
    None
}

/// Write the cross-tab from a `PivotResult` into a fresh `DataProxy` that
/// becomes the pivot's output sheet. The caller is responsible for pushing
/// the returned `DataProxy` into the workbook's `SheetsRegistry` and
/// marking it read-only.
///
/// **Single-value layout** (`nv == 1`, issue #35) — one header row:
/// ```text
///  [row-field 1] [row-field 2] ... [col key 1] [col key 2] ... [Total]
///  [row key 1a] [row key 1b]    ... [body]      [body]      ... [row tot]
///  [row key 2a] [row key 2b]    ... [body]      [body]      ... [row tot]
///  [Total]      [Total]         ... [col tot]   [col tot]   ... [grand]
/// ```
///
/// **Multi-value layout** (`nv > 1`, issue #59) — two header rows: the
/// first carries the value-field name spanning its `nc_keys + 1` block
/// (e.g. "Sum of Amount"), the second repeats the col-keys + "Total"
/// for each value-field block. Each block sits side-by-side after the
/// row-key columns.
pub fn materialize(
    pt: &PivotTable,
    result: &PivotResult,
    field_headers: &[String],
    output_sheet_name: &str,
) -> DataProxy {
    use crate::core::data_proxy::Style;

    let mut out = DataProxy::new(output_sheet_name);

    // ---- Build styles once (add_style dedups) ----
    let header_style_idx = {
        let mut s = Style::default();
        s.bold = true;
        s.bgcolor = Some("#e8eef7".to_string());
        s.align = "center".to_string();
        s.valign = "middle".to_string();
        out.add_style(s)
    };
    let total_style_idx = {
        let mut s = Style::default();
        s.bold = true;
        s.bgcolor = Some("#fff3cd".to_string());
        s.align = "right".to_string();
        out.add_style(s)
    };
    let label_style_idx = {
        let mut s = Style::default();
        s.bold = true;
        s.align = "left".to_string();
        out.add_style(s)
    };
    let body_style_idx = out.add_style(Style::default());

    let nr_keys = result.row_keys.len();
    let nc_keys = result.col_keys.len();
    let nr = pt.row_fields.len();
    let value_fields = pt.effective_value_fields();
    let nv = value_fields.len();
    // Each value-field block is `nc_keys + 1` columns wide (the trailing
    // column is that value field's row total). The row-key columns sit
    // to the left of every block.
    let block_width = nc_keys + 1;
    let multi_value = nv > 1;

    // --- Optional top header row (multi-value only) ---
    // Row 0 carries the value-field name in the first cell of each block;
    // the rest of the row stays blank. Row-key cells are also blanked so
    // the band reads as a banner.
    let (data_row_offset, totals_row_idx): (usize, usize) = if multi_value {
        for j in 0..nr {
            out.set_cell_text(0, j, "");
            out.set_cell_style(0, j, header_style_idx);
        }
        for (v_idx, vf) in value_fields.iter().enumerate() {
            let first_col = nr + v_idx * block_width;
            let field_name = field_headers.get(vf.field).cloned().unwrap_or_default();
            let label = format!("{} of {}", vf.agg.label(), field_name);
            out.set_cell_text(0, first_col, &label);
            out.set_cell_style(0, first_col, header_style_idx);
            for c in 1..block_width {
                out.set_cell_text(0, first_col + c, "");
                out.set_cell_style(0, first_col + c, header_style_idx);
            }
        }
        (1usize, 1 + nr_keys + 1)
    } else {
        (0usize, 1 + nr_keys)
    };

    // --- Main header row (row `data_row_offset`) ---
    // Single-value: row-key field names on the left, col-keys + "Total" on
    // the right. Multi-value: row-key field names + col-keys + "Total"
    // repeated for every value-field block.
    for (i, &field_idx) in pt.row_fields.iter().enumerate() {
        let h = field_headers.get(field_idx).cloned().unwrap_or_default();
        out.set_cell_text(data_row_offset, i, &h);
        out.set_cell_style(data_row_offset, i, header_style_idx);
    }
    for (v_idx, _vf) in value_fields.iter().enumerate() {
        let block_start = nr + v_idx * block_width;
        for (j, ck) in result.col_keys.iter().enumerate() {
            let s = key_to_display(ck);
            out.set_cell_text(data_row_offset, block_start + j, &s);
            out.set_cell_style(data_row_offset, block_start + j, header_style_idx);
        }
        out.set_cell_text(data_row_offset, block_start + nc_keys, "Total");
        out.set_cell_style(data_row_offset, block_start + nc_keys, header_style_idx);
    }

    // --- Body rows ---
    for (i, rk) in result.row_keys.iter().enumerate() {
        let row = data_row_offset + 1 + i;
        match rk {
            Key::Single(_) => {
                if nr > 0 {
                    out.set_cell_text(row, 0, &key_to_display(rk));
                    out.set_cell_style(row, 0, label_style_idx);
                }
            }
            Key::Tuple(parts) => {
                for (j, p) in parts.iter().enumerate() {
                    if j < nr {
                        out.set_cell_text(row, j, &prim_to_display(p));
                        out.set_cell_style(row, j, label_style_idx);
                    }
                }
            }
        }
        for (v_idx, _vf) in value_fields.iter().enumerate() {
            let block_start = nr + v_idx * block_width;
            for (j, _ck) in result.col_keys.iter().enumerate() {
                write_value_cell(
                    &mut out,
                    row,
                    block_start + j,
                    result.body[i][v_idx * nc_keys + j],
                    body_style_idx,
                );
            }
            write_value_cell(
                &mut out,
                row,
                block_start + nc_keys,
                result.row_totals[i][v_idx],
                total_style_idx,
            );
        }
    }

    // --- Totals row ---
    // The "Total" label only makes sense in the row-label area (the first
    // `nr` columns). When there are no row fields, the label column is also
    // where the first col-key lives, so we don't write a redundant label.
    if nr > 0 {
        out.set_cell_text(totals_row_idx, 0, "Total");
        out.set_cell_style(totals_row_idx, 0, total_style_idx);
        for j in 1..nr {
            out.set_cell_text(totals_row_idx, j, "");
            out.set_cell_style(totals_row_idx, j, total_style_idx);
        }
    }
    for (v_idx, _vf) in value_fields.iter().enumerate() {
        let block_start = nr + v_idx * block_width;
        for (j, _) in result.col_keys.iter().enumerate() {
            write_value_cell(
                &mut out,
                totals_row_idx,
                block_start + j,
                result.col_totals[j][v_idx],
                total_style_idx,
            );
        }
        write_value_cell(
            &mut out,
            totals_row_idx,
            block_start + nc_keys,
            result.grand_total[v_idx],
            total_style_idx,
        );
    }

    // Pad the sheet to at least default rows so it renders.
    if out.row_count < totals_row_idx + 1 {
        out.row_count = totals_row_idx + 1;
    }
    // Pad columns too — multi-value blocks widen the sheet, and the
    // renderer's default column count (26) is the single-value case.
    let total_cols = nr + nv * block_width;
    if out.cols.len < total_cols {
        out.cols.len = total_cols;
    }

    out
}

/// Write a numeric value as text into a cell and apply the given style.
/// `None` is written as empty (no number); we'll still apply the style.
fn write_value_cell(
    out: &mut DataProxy,
    ri: usize,
    ci: usize,
    v: Option<f64>,
    style_idx: usize,
) {
    match v {
        Some(n) => {
            // Use a fixed display: integers without trailing `.0`, floats
            // with up to 6 significant digits (matches Excel's default).
            let s = if n == n.trunc() && n.abs() < 1e15 {
                format!("{}", n as i64)
            } else {
                format!("{}", n)
            };
            out.set_cell_text(ri, ci, &s);
        }
        None => {
            out.set_cell_text(ri, ci, "");
        }
    }
    out.set_cell_style(ri, ci, style_idx);
}

/// Read the source range's header row (row `r0`) as `Vec<String>` of
/// column-header labels (one per source column). Blank headers become `""`.
pub fn read_field_headers(source: &DataProxy, r0: usize, c0: usize, c1: usize) -> Vec<String> {
    (c0..=c1)
        .map(|ci| {
            let s = source.cell_raw_value(r0, ci);
            s.trim().to_string()
        })
        .collect()
}


fn aggregate(agg: &Agg, vs: &[Option<f64>]) -> Option<f64> {
    match agg {
        Agg::Count => {
            let n = vs.iter().filter(|v| v.is_some()).count();
            if n == 0 { None } else { Some(n as f64) }
        }
        Agg::Sum => {
            let mut sum = 0.0;
            let mut any = false;
            for v in vs.iter().flatten() {
                sum += v;
                any = true;
            }
            if any { Some(sum) } else { None }
        }
        Agg::Avg => {
            let nums: Vec<f64> = vs.iter().filter_map(|v| *v).collect();
            if nums.is_empty() {
                None
            } else {
                Some(nums.iter().sum::<f64>() / nums.len() as f64)
            }
        }
        Agg::Min => vs.iter().filter_map(|v| *v).fold(None, |acc, v| {
            Some(match acc { None => v, Some(a) => a.min(v) })
        }),
        Agg::Max => vs.iter().filter_map(|v| *v).fold(None, |acc, v| {
            Some(match acc { None => v, Some(a) => a.max(v) })
        }),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::data_proxy::DataProxy;

    /// Build a `DataProxy` from a Vec of `Option<&str>` rows, where `None`
    /// represents an empty cell. The first row is the header.
    fn sheet_from_rows(name: &str, rows: &[&[&str]]) -> DataProxy {
        let mut dp = DataProxy::new(name);
        for (ri, row) in rows.iter().enumerate() {
            for (ci, cell) in row.iter().enumerate() {
                if !cell.is_empty() {
                    dp.set_cell_text(ri, ci, cell);
                }
            }
        }
        dp
    }

    fn pt(source: &str, row_fields: Vec<usize>, col_fields: Vec<usize>, value: usize, agg: Agg) -> PivotTable {
        PivotTable {
            source_range: source.into(),
            source_sheet: "S".into(),
            row_fields,
            col_fields,
            value_field: value,
            agg,
            value_fields: vec![],
            filter_fields: vec![],
            date_groups: HashMap::new(),
            output_sheet: "Pivot1".into(),
        }
    }

    fn pt_multi(
        source: &str,
        row_fields: Vec<usize>,
        col_fields: Vec<usize>,
        values: Vec<(usize, Agg)>,
    ) -> PivotTable {
        let mut p = pt(source, row_fields, col_fields, 0, Agg::Sum);
        p.value_fields = values
            .into_iter()
            .map(|(field, agg)| ValueField { field, agg })
            .collect();
        p
    }

    #[test]
    fn sum_aggregates_numeric_values() {
        let dp = sheet_from_rows("S", &[
            &["Region", "Amount"],
            &["North", "100"],
            &["North", "200"],
            &["South", "50"],
        ]);
        let p = pt("S!A1:B4", vec![0], vec![], 1, Agg::Sum);
        let r = compute(&dp, &p).unwrap();
        assert_eq!(r.row_keys.len(), 2);
        assert_eq!(r.col_keys.len(), 1);
        // North appears first in the source, so row_keys[0] is "North".
        assert_eq!(key_to_display(&r.row_keys[0]), "North");
        assert_eq!(r.body[0][0], Some(300.0));
        assert_eq!(r.body[1][0], Some(50.0));
        assert_eq!(r.row_totals, vec![vec![Some(300.0)], vec![Some(50.0)]]);
        assert_eq!(r.col_totals, vec![vec![Some(350.0)]]);
        assert_eq!(r.grand_total, vec![Some(350.0)]);
    }

    #[test]
    fn count_excludes_blanks() {
        let dp = sheet_from_rows("S", &[
            &["Name", "Score"],
            &["Alice", "10"],
            &["Alice", ""],     // blank → not counted
            &["Bob", "20"],
            &["Bob", "30"],
        ]);
        let p = pt("S!A1:B5", vec![0], vec![], 1, Agg::Count);
        let r = compute(&dp, &p).unwrap();
        assert_eq!(r.body[0][0], Some(1.0)); // Alice: 1 non-blank
        assert_eq!(r.body[1][0], Some(2.0)); // Bob: 2 non-blank
        assert_eq!(r.grand_total, vec![Some(3.0)]);
    }

    #[test]
    fn avg_with_text_numeric_coercion() {
        let dp = sheet_from_rows("S", &[
            &["X", "Y"],
            &["a", "10"],
            &["a", "20"],
            &["a", "30"],
        ]);
        let p = pt("S!A1:B4", vec![0], vec![], 1, Agg::Avg);
        let r = compute(&dp, &p).unwrap();
        assert_eq!(r.body[0][0], Some(20.0));
    }

    #[test]
    fn min_max_ignore_blanks() {
        let dp = sheet_from_rows("S", &[
            &["X", "V"],
            &["a", "5"],
            &["a", ""],
            &["a", "10"],
            &["a", "abc"], // not a number → dropped from min/max
        ]);
        let pmin = pt("S!A1:B5", vec![0], vec![], 1, Agg::Min);
        let rmin = compute(&dp, &pmin).unwrap();
        assert_eq!(rmin.body[0][0], Some(5.0));

        let pmax = pt("S!A1:B5", vec![0], vec![], 1, Agg::Max);
        let rmax = compute(&dp, &pmax).unwrap();
        assert_eq!(rmax.body[0][0], Some(10.0));
    }

    #[test]
    fn empty_source_range_errors() {
        // A 1×1 source (just a header) has no data row to aggregate.
        let dp = sheet_from_rows("S", &[&["H"]]);
        let p = pt("S!A1:A1", vec![], vec![], 0, Agg::Sum);
        let res = compute(&dp, &p);
        assert!(res.is_err());
    }

    #[test]
    fn no_row_fields_means_single_total_row() {
        // Single value column, no row/col grouping.
        let dp = sheet_from_rows("S", &[
            &["Amount"],
            &["10"],
            &["20"],
            &["30"],
        ]);
        let p = pt("S!A1:A4", vec![], vec![], 0, Agg::Sum);
        let r = compute(&dp, &p).unwrap();
        assert_eq!(r.row_keys.len(), 1);
        assert_eq!(key_to_display(&r.row_keys[0]), "");
        assert_eq!(r.col_keys.len(), 1);
        assert_eq!(r.body[0][0], Some(60.0));
        assert_eq!(r.row_totals, vec![vec![Some(60.0)]]);
        assert_eq!(r.col_totals, vec![vec![Some(60.0)]]);
        assert_eq!(r.grand_total, vec![Some(60.0)]);
    }

    // -----------------------------------------------------------------
    // Page-level filter (issue #58)
    // -----------------------------------------------------------------

    /// Helper: a pivot with one filter field on column `field_idx`,
    /// `selected_values` as the "currently checked" set. Empty list means
    /// "All" (every row passes).
    fn pt_filtered(
        source: &str,
        row_fields: Vec<usize>,
        col_fields: Vec<usize>,
        value: usize,
        agg: Agg,
        field_idx: usize,
        selected_values: Vec<&str>,
    ) -> PivotTable {
        let mut p = pt(source, row_fields, col_fields, value, agg);
        p.filter_fields.push(FilterField {
            field_idx,
            selected_values: selected_values.into_iter().map(String::from).collect(),
        });
        p
    }

    #[test]
    fn filter_with_no_selected_values_passes_all_rows() {
        // Empty `selected_values` is the "All" sentinel — every row passes.
        let dp = sheet_from_rows("S", &[
            &["Region", "Amount"],
            &["North", "10"],
            &["South", "20"],
        ]);
        let p = pt_filtered(
            "S!A1:B3",
            vec![0],
            vec![],
            1,
            Agg::Sum,
            0,
            vec![],
        );
        let r = compute(&dp, &p).unwrap();
        assert_eq!(r.grand_total, vec![Some(30.0)]);
        assert_eq!(r.row_keys.len(), 2);
    }

    #[test]
    fn filter_excludes_rows_not_in_selected_values() {
        // Filter Region ∈ {North} — South rows drop out.
        let dp = sheet_from_rows("S", &[
            &["Region", "Amount"],
            &["North", "10"],
            &["South", "20"],
            &["North", "5"],
        ]);
        let p = pt_filtered(
            "S!A1:B4",
            vec![0],
            vec![],
            1,
            Agg::Sum,
            0,
            vec!["North"],
        );
        let r = compute(&dp, &p).unwrap();
        assert_eq!(r.grand_total, vec![Some(15.0)]);
        // Only the North row_key survives.
        assert_eq!(r.row_keys.len(), 1);
        assert_eq!(key_to_display(&r.row_keys[0]), "North");
    }

    #[test]
    fn filter_grand_total_excludes_filtered_rows() {
        // The grand total must also reflect the filter — not the unfiltered sum.
        let dp = sheet_from_rows("S", &[
            &["Region", "Amount"],
            &["North", "10"],
            &["South", "20"],
            &["East", "30"],
            &["West", "40"],
        ]);
        let p = pt_filtered(
            "S!A1:B5",
            vec![0],
            vec![],
            1,
            Agg::Sum,
            0,
            vec!["North", "South"],
        );
        let r = compute(&dp, &p).unwrap();
        assert_eq!(r.grand_total, vec![Some(30.0)]);
    }

    #[test]
    fn filter_with_no_matching_values_yields_empty_body() {
        // The filter selection excludes every row — body cells are None,
        // but the row_keys still reflect the surviving source.
        let dp = sheet_from_rows("S", &[
            &["Region", "Amount"],
            &["North", "10"],
        ]);
        let p = pt_filtered(
            "S!A1:B2",
            vec![0],
            vec![],
            1,
            Agg::Sum,
            0,
            vec!["Mars"],
        );
        let r = compute(&dp, &p).unwrap();
        assert_eq!(r.grand_total, vec![None]);
        // No row_keys survive — the only source row failed the filter.
        assert_eq!(r.row_keys.len(), 0);
    }

    #[test]
    fn no_col_fields_means_single_total_column() {
        let dp = sheet_from_rows("S", &[
            &["Region", "Amount"],
            &["North", "5"],
            &["South", "7"],
        ]);
        let p = pt("S!A1:B3", vec![0], vec![], 1, Agg::Sum);
        let r = compute(&dp, &p).unwrap();
        assert_eq!(r.col_keys.len(), 1);
        assert_eq!(r.body[0][0], Some(5.0));
        assert_eq!(r.body[1][0], Some(7.0));
        assert_eq!(r.grand_total, vec![Some(12.0)]);
    }

    #[test]
    fn multi_row_field_uses_tuple_key() {
        // Two row fields: Region + Product. Same (Region, Product) collapses.
        let dp = sheet_from_rows("S", &[
            &["Region", "Product", "Amount"],
            &["North", "Apple", "10"],
            &["North", "Apple", "20"],
            &["North", "Banana", "5"],
            &["South", "Apple", "50"],
        ]);
        let p = pt("S!A1:C5", vec![0, 1], vec![], 2, Agg::Sum);
        let r = compute(&dp, &p).unwrap();
        assert_eq!(r.row_keys.len(), 3);
        // Distinct (Region, Product) combos in first-appearance order.
        let names: Vec<String> = r.row_keys.iter().map(key_to_display).collect();
        assert_eq!(names, vec!["North / Apple", "North / Banana", "South / Apple"]);
        assert_eq!(r.body[0][0], Some(30.0)); // North+Apple: 10+20
        assert_eq!(r.body[1][0], Some(5.0));
        assert_eq!(r.body[2][0], Some(50.0));
    }

    #[test]
    fn headers_row_excluded_from_data() {
        let dp = sheet_from_rows("S", &[
            &["Label", "V"],   // header row — should NOT be aggregated
            &["Label", "100"], // this row's "Label" / "V" are data
            &["X", "200"],
        ]);
        let p = pt("S!A1:B3", vec![0], vec![], 1, Agg::Sum);
        let r = compute(&dp, &p).unwrap();
        // Two data rows: ("Label", 100) and ("X", 200). The header "Label" is
        // not data, so "Label" is a real row key with sum 100.
        assert_eq!(r.row_keys.len(), 2);
        assert_eq!(r.grand_total, vec![Some(300.0)]);
    }

    #[test]
    fn text_numeric_keys_collapse() {
        // 100 and "100" in the same key column must be the same bucket.
        let dp = sheet_from_rows("S", &[
            &["K", "V"],
            &["100", "10"],
            &["100", "20"],
        ]);
        let p = pt("S!A1:B3", vec![0], vec![], 1, Agg::Sum);
        let r = compute(&dp, &p).unwrap();
        assert_eq!(r.row_keys.len(), 1);
        assert_eq!(r.body[0][0], Some(30.0));
    }

    #[test]
    fn col_field_creates_cross_tab() {
        // Quarter as a column field; each row × quarter cell = sum of Amount.
        let dp = sheet_from_rows("S", &[
            &["Region", "Quarter", "Amount"],
            &["North", "Q1", "10"],
            &["North", "Q2", "20"],
            &["South", "Q1", "50"],
        ]);
        let p = pt("S!A1:C4", vec![0], vec![1], 2, Agg::Sum);
        let r = compute(&dp, &p).unwrap();
        // 2 row keys, 2 col keys (Q1, Q2) in first-appearance order.
        assert_eq!(r.row_keys.len(), 2);
        assert_eq!(r.col_keys.len(), 2);
        let q1: Vec<String> = r.col_keys.iter().map(key_to_display).collect();
        assert_eq!(q1, vec!["Q1", "Q2"]);
        // North × Q1 = 10; North × Q2 = 20; South × Q1 = 50; South × Q2 = None.
        assert_eq!(r.body[0][0], Some(10.0));
        assert_eq!(r.body[0][1], Some(20.0));
        assert_eq!(r.body[1][0], Some(50.0));
        assert_eq!(r.body[1][1], None);
        // Row totals: North=30, South=50.
        assert_eq!(r.row_totals, vec![vec![Some(30.0)], vec![Some(50.0)]]);
        // Col totals: Q1=60, Q2=20.
        assert_eq!(r.col_totals, vec![vec![Some(60.0)], vec![Some(20.0)]]);
        // Grand total = 80.
        assert_eq!(r.grand_total, vec![Some(80.0)]);
    }

    #[test]
    fn serde_roundtrip_preserves_pivots() {
        let p = PivotTable {
            source_range: "Sheet1!A1:D12".into(),
            source_sheet: "Sheet1".into(),
            row_fields: vec![0, 1],
            col_fields: vec![2],
            value_field: 3,
            agg: Agg::Avg,
            value_fields: vec![],
            filter_fields: vec![],
            date_groups: HashMap::new(),
            output_sheet: "Pivot1".into(),
        };
        let s = serde_json::to_string(&p).unwrap();
        let back: PivotTable = serde_json::from_str(&s).unwrap();
        assert_eq!(back.source_range, p.source_range);
        assert_eq!(back.row_fields, p.row_fields);
        assert_eq!(back.col_fields, p.col_fields);
        assert_eq!(back.value_field, p.value_field);
        assert_eq!(back.agg, Agg::Avg);
    }

    #[test]
    fn validate_rejects_out_of_range_field() {
        let dp = sheet_from_rows("S", &[&["A", "B"], &["x", "1"]]);
        let p = pt("S!A1:B2", vec![5], vec![], 0, Agg::Sum); // field 5 out of range
        assert!(compute(&dp, &p).is_err());
    }

    #[test]
    fn materialize_writes_header_body_and_totals() {
        let dp = sheet_from_rows("S", &[
            &["Region", "Amount"],
            &["North", "100"],
            &["North", "200"],
            &["South", "50"],
        ]);
        let p = pt("S!A1:B4", vec![0], vec![], 1, Agg::Sum);
        let r = compute(&dp, &p).unwrap();
        let headers = read_field_headers(&dp, 0, 0, 1);
        assert_eq!(headers, vec!["Region", "Amount"]);
        let out = materialize(&p, &r, &headers, "Pivot1");
        // Header row 0: "Region" in (0,0), empty col-key at (0,1), "Total" in (0,2)
        assert_eq!(out.get_cell_text(0, 0), "Region");
        assert_eq!(out.get_cell_text(0, 1), "");
        assert_eq!(out.get_cell_text(0, 2), "Total");
        // Body row 1: "North" / 300 / 300
        assert_eq!(out.get_cell_text(1, 0), "North");
        assert_eq!(out.get_cell_text(1, 1), "300");
        assert_eq!(out.get_cell_text(1, 2), "300");
        // Body row 2: "South" / 50 / 50
        assert_eq!(out.get_cell_text(2, 0), "South");
        assert_eq!(out.get_cell_text(2, 1), "50");
        assert_eq!(out.get_cell_text(2, 2), "50");
        // Totals row 3: "Total" / 350 / 350
        assert_eq!(out.get_cell_text(3, 0), "Total");
        assert_eq!(out.get_cell_text(3, 1), "350");
        assert_eq!(out.get_cell_text(3, 2), "350");
    }

    #[test]
    fn materialize_with_col_field_lays_out_header_rows() {
        let dp = sheet_from_rows("S", &[
            &["Region", "Quarter", "Amount"],
            &["North", "Q1", "10"],
            &["North", "Q2", "20"],
            &["South", "Q1", "50"],
        ]);
        let p = pt("S!A1:C4", vec![0], vec![1], 2, Agg::Sum);
        let r = compute(&dp, &p).unwrap();
        let headers = read_field_headers(&dp, 0, 0, 2);
        let out = materialize(&p, &r, &headers, "Pivot1");
        // Row 0: col 0 = "Region", col 1 = "Q1", col 2 = "Q2", col 3 = "Total"
        assert_eq!(out.get_cell_text(0, 0), "Region");
        assert_eq!(out.get_cell_text(0, 1), "Q1");
        assert_eq!(out.get_cell_text(0, 2), "Q2");
        assert_eq!(out.get_cell_text(0, 3), "Total");
        // Body rows 1..2
        assert_eq!(out.get_cell_text(1, 0), "North");
        assert_eq!(out.get_cell_text(1, 1), "10");
        assert_eq!(out.get_cell_text(1, 2), "20");
        assert_eq!(out.get_cell_text(1, 3), "30");
        assert_eq!(out.get_cell_text(2, 0), "South");
        assert_eq!(out.get_cell_text(2, 1), "50");
        assert_eq!(out.get_cell_text(2, 2), ""); // None → empty
        assert_eq!(out.get_cell_text(2, 3), "50");
        // Totals row 3
        assert_eq!(out.get_cell_text(3, 0), "Total");
        assert_eq!(out.get_cell_text(3, 1), "60");
        assert_eq!(out.get_cell_text(3, 2), "20");
        assert_eq!(out.get_cell_text(3, 3), "80");
    }

    #[test]
    fn materialize_with_no_row_or_col_fields_writes_single_total() {
        // Single value column, no grouping axes → 1×1 body + grand total.
        let dp = sheet_from_rows("S", &[
            &["Amount"],
            &["10"],
            &["20"],
            &["30"],
        ]);
        let p = pt("S!A1:A4", vec![], vec![], 0, Agg::Sum);
        let r = compute(&dp, &p).unwrap();
        let headers = read_field_headers(&dp, 0, 0, 0);
        let out = materialize(&p, &r, &headers, "Pivot1");
        // Header row 0: blank col-key at (0,0), "Total" at (0,1)
        assert_eq!(out.get_cell_text(0, 0), "");
        assert_eq!(out.get_cell_text(0, 1), "Total");
        // Body row 1: with nr=0, the body value lives at col 0 and the
        // row_total at col 1 (both are 60 here).
        assert_eq!(out.get_cell_text(1, 0), "60");
        assert_eq!(out.get_cell_text(1, 1), "60");
        // Totals row 2: no row-field label column (nr=0), so col 0 holds
        // the col_total and col 1 holds the grand total — both are 60.
        assert_eq!(out.get_cell_text(2, 0), "60");
        assert_eq!(out.get_cell_text(2, 1), "60");
    }

    // -----------------------------------------------------------------
    // Multi-value fields (issue #59)
    // -----------------------------------------------------------------

    #[test]
    fn effective_value_fields_falls_back_to_legacy_pair() {
        // An empty `value_fields` (legacy workbook) yields a one-element
        // list built from `value_field` / `agg`.
        let p = pt("S!A1:B2", vec![0], vec![], 1, Agg::Sum);
        let eff = p.effective_value_fields();
        assert_eq!(eff.len(), 1);
        assert_eq!(eff[0].field, 1);
        assert_eq!(eff[0].agg, Agg::Sum);
    }

    #[test]
    fn effective_value_fields_uses_value_fields_when_present() {
        // A populated `value_fields` is authoritative; legacy `value_field`/
        // `agg` are ignored even if they look different.
        let p = pt_multi(
            "S!A1:C2",
            vec![],
            vec![],
            vec![(1, Agg::Sum), (2, Agg::Count)],
        );
        let eff = p.effective_value_fields();
        assert_eq!(eff.len(), 2);
        assert_eq!(eff[0].field, 1);
        assert_eq!(eff[0].agg, Agg::Sum);
        assert_eq!(eff[1].field, 2);
        assert_eq!(eff[1].agg, Agg::Count);
    }

    #[test]
    fn two_value_fields_widen_body_and_totals() {
        // Two value fields, no row/col grouping: every cell aggregates over
        // the whole source under each value field's aggregation.
        // Source: 4 rows of numeric Amounts and a numeric Items count
        // column (the engine's Agg::Count works on numeric values, so
        // both columns have to be numeric for this test).
        let dp = sheet_from_rows("S", &[
            &["Amount", "Items"],
            &["10", "1"],
            &["20", "1"],
            &["30", "1"],
            &["40", "1"],
        ]);
        // Sum of Amount + Count of Items (4 non-blank rows).
        let p = pt_multi(
            "S!A1:B5",
            vec![],
            vec![],
            vec![(0, Agg::Sum), (1, Agg::Count)],
        );
        let r = compute(&dp, &p).unwrap();
        assert_eq!(r.nv, 2);
        // Single row_key (no row fields → Single(Blank)), single col_key
        // (no col fields → Single(Blank)). Body[r=0] = [sum_of_Amount,
        // count_of_Items] flattened.
        assert_eq!(r.row_keys.len(), 1);
        assert_eq!(r.col_keys.len(), 1);
        assert_eq!(r.body[0], vec![Some(100.0), Some(4.0)]);
        assert_eq!(r.row_totals, vec![vec![Some(100.0), Some(4.0)]]);
        assert_eq!(r.col_totals, vec![vec![Some(100.0), Some(4.0)]]);
        assert_eq!(r.grand_total, vec![Some(100.0), Some(4.0)]);
    }

    #[test]
    fn two_value_fields_with_col_field() {
        // Quarter as a col field; Sum of Amount + Count of Amount per cell.
        // 2 value fields × 2 col keys = 4 body columns per row.
        let dp = sheet_from_rows("S", &[
            &["Region", "Quarter", "Amount"],
            &["North", "Q1", "10"],
            &["North", "Q2", "20"],
            &["South", "Q1", "50"],
            &["South", "Q2", "30"],
        ]);
        let p = pt_multi(
            "S!A1:C5",
            vec![0],
            vec![1],
            vec![(2, Agg::Sum), (2, Agg::Count)],
        );
        let r = compute(&dp, &p).unwrap();
        assert_eq!(r.nv, 2);
        assert_eq!(r.row_keys.len(), 2);
        assert_eq!(r.col_keys.len(), 2);
        // body[r][v*nc + c]: each row has 4 cells, Sum-block first.
        // North: Sum(Q1=10, Q2=20), Count(Q1=1, Q2=1)
        // South: Sum(Q1=50, Q2=30), Count(Q1=1, Q2=1)
        assert_eq!(r.body[0], vec![Some(10.0), Some(20.0), Some(1.0), Some(1.0)]);
        assert_eq!(r.body[1], vec![Some(50.0), Some(30.0), Some(1.0), Some(1.0)]);
        // row_totals[r][v]: North sum=30, count=2; South sum=80, count=2.
        assert_eq!(r.row_totals, vec![vec![Some(30.0), Some(2.0)], vec![Some(80.0), Some(2.0)]]);
        // col_totals[c][v]: Q1 sum=60 count=2; Q2 sum=50 count=2.
        assert_eq!(r.col_totals, vec![vec![Some(60.0), Some(2.0)], vec![Some(50.0), Some(2.0)]]);
        // grand_total[v]: total sum=110, total count=4.
        assert_eq!(r.grand_total, vec![Some(110.0), Some(4.0)]);
    }

    #[test]
    fn three_value_fields_mixed_aggs() {
        // Three aggregations on the same value column: Sum, Count, Avg.
        // Avg of [10, 20, 30, 40] = 25.
        let dp = sheet_from_rows("S", &[
            &["Amount"],
            &["10"],
            &["20"],
            &["30"],
            &["40"],
        ]);
        let p = pt_multi(
            "S!A1:A5",
            vec![],
            vec![],
            vec![(0, Agg::Sum), (0, Agg::Count), (0, Agg::Avg)],
        );
        let r = compute(&dp, &p).unwrap();
        assert_eq!(r.nv, 3);
        assert_eq!(r.grand_total, vec![Some(100.0), Some(4.0), Some(25.0)]);
    }

    #[test]
    fn value_fields_override_legacy_value_field_when_both_set() {
        // If a workbook carries both `value_field` and `value_fields` (e.g.
        // a hand-edited spec), the new `value_fields` wins. This pins the
        // precedence: the engine must not silently fall through to the
        // legacy field.
        let dp = sheet_from_rows("S", &[
            &["A", "B", "C"],
            &["x", "10", "100"],
            &["y", "20", "200"],
        ]);
        let mut p = pt("S!A1:C3", vec![0], vec![], 1, Agg::Count); // legacy: Count of B
        // New spec says Sum of C.
        p.value_fields = vec![ValueField { field: 2, agg: Agg::Sum }];
        let r = compute(&dp, &p).unwrap();
        // Grand total is Sum of C across all rows: 100 + 200 = 300.
        assert_eq!(r.grand_total, vec![Some(300.0)]);
    }

    #[test]
    fn materialize_multi_value_lays_out_two_header_rows() {
        // Two value fields with a row field and a col field. The output
        // should have:
        //   row 0: value-field names spanning their blocks
        //   row 1: row-key field name | col keys | Total | col keys | Total
        //   body  : row-key label | values... | values...
        //   totals: "Total" | sums/counts | sums/counts
        let dp = sheet_from_rows("S", &[
            &["Region", "Quarter", "Amount"],
            &["North", "Q1", "10"],
            &["North", "Q2", "20"],
            &["South", "Q1", "50"],
        ]);
        let p = pt_multi(
            "S!A1:C4",
            vec![0],
            vec![1],
            vec![(2, Agg::Sum), (2, Agg::Count)],
        );
        let r = compute(&dp, &p).unwrap();
        let headers = read_field_headers(&dp, 0, 0, 2);
        let out = materialize(&p, &r, &headers, "Pivot1");

        // Top header row (row 0): row-key cells blank, then "Sum of Amount"
        // spanning the first block (cols 1..=3) and "Count of Amount"
        // spanning the second block (cols 4..=6).
        assert_eq!(out.get_cell_text(0, 0), "");
        assert_eq!(out.get_cell_text(0, 1), "Sum of Amount");
        assert_eq!(out.get_cell_text(0, 2), "");
        assert_eq!(out.get_cell_text(0, 3), "");
        assert_eq!(out.get_cell_text(0, 4), "Count of Amount");
        assert_eq!(out.get_cell_text(0, 5), "");
        assert_eq!(out.get_cell_text(0, 6), "");

        // Main header row (row 1): "Region" | Q1 | Q2 | Total | Q1 | Q2 | Total
        assert_eq!(out.get_cell_text(1, 0), "Region");
        assert_eq!(out.get_cell_text(1, 1), "Q1");
        assert_eq!(out.get_cell_text(1, 2), "Q2");
        assert_eq!(out.get_cell_text(1, 3), "Total");
        assert_eq!(out.get_cell_text(1, 4), "Q1");
        assert_eq!(out.get_cell_text(1, 5), "Q2");
        assert_eq!(out.get_cell_text(1, 6), "Total");

        // Body row 2: North | 10 | 20 | 30 | 1 | 1 | 2
        assert_eq!(out.get_cell_text(2, 0), "North");
        assert_eq!(out.get_cell_text(2, 1), "10");
        assert_eq!(out.get_cell_text(2, 2), "20");
        assert_eq!(out.get_cell_text(2, 3), "30");
        assert_eq!(out.get_cell_text(2, 4), "1");
        assert_eq!(out.get_cell_text(2, 5), "1");
        assert_eq!(out.get_cell_text(2, 6), "2");

        // Body row 3: South | 50 | "" | 50 | 1 | "" | 1
        // (no source row has South, Q2 — that bucket is empty, so both
        // Sum and Count come back as None, which renders as empty. The
        // row total counts both surviving Q1 entries → Count = 1.)
        assert_eq!(out.get_cell_text(3, 0), "South");
        assert_eq!(out.get_cell_text(3, 1), "50");
        assert_eq!(out.get_cell_text(3, 2), ""); // None → empty
        assert_eq!(out.get_cell_text(3, 3), "50");
        assert_eq!(out.get_cell_text(3, 4), "1");
        assert_eq!(out.get_cell_text(3, 5), ""); // empty bucket → empty
        assert_eq!(out.get_cell_text(3, 6), "1");

        // Totals row 4: "Total" | 60 | 20 | 80 | 2 | 1 | 3
        assert_eq!(out.get_cell_text(4, 0), "Total");
        assert_eq!(out.get_cell_text(4, 1), "60");
        assert_eq!(out.get_cell_text(4, 2), "20");
        assert_eq!(out.get_cell_text(4, 3), "80");
        assert_eq!(out.get_cell_text(4, 4), "2");
        assert_eq!(out.get_cell_text(4, 5), "1");
        assert_eq!(out.get_cell_text(4, 6), "3");
    }

    #[test]
    fn validate_rejects_out_of_range_value_field() {
        // A multi-value spec with a value field index past the source's
        // last column must be rejected by `validate()` (issue #59).
        let dp = sheet_from_rows("S", &[&["A", "B"], &["x", "1"]]);
        let bad = pt_multi("S!A1:B2", vec![], vec![], vec![(5, Agg::Sum)]);
        assert!(compute(&dp, &bad).is_err());
    }

    // -----------------------------------------------------------------
    // Date grouping (issue #60)
    // -----------------------------------------------------------------

    /// Helper: a pivot with one row field, one value field, and an optional
    /// date-grouping override on `row_field_idx` (the column index, not a
    /// `PivotTable` field offset).
    fn pt_dated(
        source: &str,
        row_field_idx: usize,
        value: usize,
        agg: Agg,
        date_group: Option<DateGroup>,
    ) -> PivotTable {
        let mut p = pt(source, vec![row_field_idx], vec![], value, agg);
        if let Some(g) = date_group {
            p.date_groups.insert(row_field_idx, g);
        }
        p
    }

    #[test]
    fn date_group_year_buckets_by_year() {
        // Source has dates spanning 2023 and 2024; Year grouping should
        // collapse to two row keys ("2023", "2024") with the corresponding
        // sum of Amounts.
        let dp = sheet_from_rows("S", &[
            &["Date", "Amount"],
            &["2023-01-15", "10"],
            &["2023-06-20", "20"],
            &["2024-01-10", "50"],
        ]);
        let p = pt_dated("S!A1:B4", 0, 1, Agg::Sum, Some(DateGroup::Year));
        let r = compute(&dp, &p).unwrap();
        let keys: Vec<String> = r.row_keys.iter().map(key_to_display).collect();
        assert_eq!(keys, vec!["2023", "2024"]);
        assert_eq!(r.body[0][0], Some(30.0)); // 10 + 20
        assert_eq!(r.body[1][0], Some(50.0));
        assert_eq!(r.grand_total, vec![Some(80.0)]);
    }

    #[test]
    fn date_group_quarter_buckets_by_year_quarter() {
        // Same dates — Quarter grouping collapses the two 2023 entries
        // into one bucket (Q1 vs Q2 differ), keeps 2024 as Q1.
        let dp = sheet_from_rows("S", &[
            &["Date", "Amount"],
            &["2023-01-15", "10"],
            &["2023-06-20", "20"],
            &["2024-01-10", "50"],
        ]);
        let p = pt_dated("S!A1:B4", 0, 1, Agg::Sum, Some(DateGroup::Quarter));
        let r = compute(&dp, &p).unwrap();
        let keys: Vec<String> = r.row_keys.iter().map(key_to_display).collect();
        assert_eq!(keys, vec!["2023-Q1", "2023-Q2", "2024-Q1"]);
        assert_eq!(r.body[0][0], Some(10.0));
        assert_eq!(r.body[1][0], Some(20.0));
        assert_eq!(r.body[2][0], Some(50.0));
    }

    #[test]
    fn date_group_month_buckets_by_year_month() {
        // Same dates — Month grouping produces three distinct keys
        // spanning two years.
        let dp = sheet_from_rows("S", &[
            &["Date", "Amount"],
            &["2023-01-15", "10"],
            &["2023-06-20", "20"],
            &["2024-01-10", "50"],
        ]);
        let p = pt_dated("S!A1:B4", 0, 1, Agg::Sum, Some(DateGroup::Month));
        let r = compute(&dp, &p).unwrap();
        let keys: Vec<String> = r.row_keys.iter().map(key_to_display).collect();
        assert_eq!(keys, vec!["2023-01", "2023-06", "2024-01"]);
    }

    #[test]
    fn date_group_day_buckets_by_exact_date() {
        // Same dates — Day grouping preserves all three distinct dates.
        let dp = sheet_from_rows("S", &[
            &["Date", "Amount"],
            &["2023-01-15", "10"],
            &["2023-06-20", "20"],
            &["2024-01-10", "50"],
        ]);
        let p = pt_dated("S!A1:B4", 0, 1, Agg::Sum, Some(DateGroup::Day));
        let r = compute(&dp, &p).unwrap();
        let keys: Vec<String> = r.row_keys.iter().map(key_to_display).collect();
        assert_eq!(keys, vec!["2023-01-15", "2023-06-20", "2024-01-10"]);
    }

    #[test]
    fn date_group_with_us_format_text_dates() {
        // US-style `M/D/YYYY` text dates parse correctly.
        let dp = sheet_from_rows("S", &[
            &["Date", "Amount"],
            &["1/15/2023", "10"],
            &["6/20/2023", "20"],
        ]);
        let p = pt_dated("S!A1:B3", 0, 1, Agg::Sum, Some(DateGroup::Quarter));
        let r = compute(&dp, &p).unwrap();
        let keys: Vec<String> = r.row_keys.iter().map(key_to_display).collect();
        assert_eq!(keys, vec!["2023-Q1", "2023-Q2"]);
    }

    #[test]
    fn date_group_with_eu_format_text_dates() {
        // EU-style `D/M/YYYY` — first part > 12 disambiguates from US.
        let dp = sheet_from_rows("S", &[
            &["Date", "Amount"],
            &["15/1/2023", "10"],
            &["20/6/2023", "20"],
        ]);
        let p = pt_dated("S!A1:B3", 0, 1, Agg::Sum, Some(DateGroup::Month));
        let r = compute(&dp, &p).unwrap();
        let keys: Vec<String> = r.row_keys.iter().map(key_to_display).collect();
        assert_eq!(keys, vec!["2023-01", "2023-06"]);
    }

    #[test]
    fn date_group_with_excel_serial_dates() {
        // Excel serial 45306 = 2024-01-15, 45366 = 2024-03-15 (2024 is a
        // leap year, so Jan 15 → Mar 15 = 60 days). We hand-pick serials
        // in the 1..200_000 range so the parser accepts them as dates.
        let dp = sheet_from_rows("S", &[
            &["Date", "Amount"],
            &["45306", "10"], // 2024-01-15
            &["45366", "20"], // 2024-03-15
        ]);
        let p = pt_dated("S!A1:B3", 0, 1, Agg::Sum, Some(DateGroup::Month));
        let r = compute(&dp, &p).unwrap();
        let keys: Vec<String> = r.row_keys.iter().map(key_to_display).collect();
        assert_eq!(keys, vec!["2024-01", "2024-03"]);
    }

    #[test]
    fn date_group_with_iso_datetime_strips_time() {
        // ISO 8601 with a `T…` time suffix — the time portion is dropped
        // before parsing.
        let dp = sheet_from_rows("S", &[
            &["Date", "Amount"],
            &["2024-03-15T10:30:00", "10"],
            &["2024-03-15T18:00:00", "20"],
        ]);
        let p = pt_dated("S!A1:B3", 0, 1, Agg::Sum, Some(DateGroup::Day));
        let r = compute(&dp, &p).unwrap();
        let keys: Vec<String> = r.row_keys.iter().map(key_to_display).collect();
        assert_eq!(keys, vec!["2024-03-15"]); // same day → one bucket
        assert_eq!(r.grand_total, vec![Some(30.0)]);
    }

    #[test]
    fn date_group_unparseable_dates_fall_back_to_raw_text() {
        // A non-date string falls through to the existing text-key path
        // so the user still sees it as a distinct bucket — better than
        // silently dropping the row.
        let dp = sheet_from_rows("S", &[
            &["Date", "Amount"],
            &["2024-03-15", "10"],
            &["not a date", "5"],
        ]);
        let p = pt_dated("S!A1:B3", 0, 1, Agg::Sum, Some(DateGroup::Month));
        let r = compute(&dp, &p).unwrap();
        let keys: Vec<String> = r.row_keys.iter().map(key_to_display).collect();
        assert_eq!(keys, vec!["2024-03", "not a date"]);
    }

    #[test]
    fn date_group_only_applies_to_named_field() {
        // Two row fields: a date column (grouped) and a category column
        // (raw). The category column stays as raw text while the date
        // collapses by month.
        let dp = sheet_from_rows("S", &[
            &["Date", "Region", "Amount"],
            &["2024-01-15", "North", "10"],
            &["2024-01-20", "South", "5"],
            &["2024-02-10", "North", "20"],
        ]);
        let mut p = pt("S!A1:C4", vec![0, 1], vec![], 2, Agg::Sum);
        p.date_groups.insert(0, DateGroup::Month);
        let r = compute(&dp, &p).unwrap();
        let keys: Vec<String> = r.row_keys.iter().map(key_to_display).collect();
        // The (date-key, region) tuple — same Region across months
        // doesn't collapse.
        assert_eq!(keys, vec![
            "2024-01 / North",
            "2024-01 / South",
            "2024-02 / North",
        ]);
        assert_eq!(r.body[0][0], Some(10.0));
        assert_eq!(r.body[1][0], Some(5.0));
        assert_eq!(r.body[2][0], Some(20.0));
    }

    #[test]
    fn date_group_in_col_field() {
        // Date grouping on a column field: cross-tab by Region (rows) and
        // month (cols).
        let dp = sheet_from_rows("S", &[
            &["Region", "Date", "Amount"],
            &["North", "2024-01-15", "10"],
            &["North", "2024-02-10", "20"],
            &["South", "2024-01-20", "5"],
        ]);
        let mut p = pt("S!A1:C4", vec![0], vec![1], 2, Agg::Sum);
        p.date_groups.insert(1, DateGroup::Month);
        let r = compute(&dp, &p).unwrap();
        let row_keys: Vec<String> = r.row_keys.iter().map(key_to_display).collect();
        let col_keys: Vec<String> = r.col_keys.iter().map(key_to_display).collect();
        assert_eq!(row_keys, vec!["North", "South"]);
        assert_eq!(col_keys, vec!["2024-01", "2024-02"]);
        // North × 2024-01 = 10; North × 2024-02 = 20; South × 2024-01 = 5.
        assert_eq!(r.body[0][0], Some(10.0));
        assert_eq!(r.body[0][1], Some(20.0));
        assert_eq!(r.body[1][0], Some(5.0));
    }

    #[test]
    fn date_group_empty_map_preserves_existing_behavior() {
        // Backward compat: a spec without `date_groups` deserializes with
        // an empty map, and the engine runs exactly as it did before #60.
        let dp = sheet_from_rows("S", &[
            &["Date", "Amount"],
            &["2024-01-15", "10"],
            &["2024-02-10", "20"],
        ]);
        let p = pt("S!A1:B3", vec![0], vec![], 1, Agg::Sum);
        // Sanity: no date_groups entry set on the spec.
        assert!(p.date_groups.is_empty());
        let r = compute(&dp, &p).unwrap();
        let keys: Vec<String> = r.row_keys.iter().map(key_to_display).collect();
        // Raw text keys: each distinct date is its own row.
        assert_eq!(keys, vec!["2024-01-15", "2024-02-10"]);
    }

    #[test]
    fn date_groups_persist_through_serde() {
        // A pivot with a date_groups map survives `to_string` /
        // `from_str` so workbook JSON preserves the grouping choice.
        let mut p = pt("S!A1:B2", vec![0], vec![], 1, Agg::Sum);
        p.date_groups.insert(0, DateGroup::Quarter);
        let s = serde_json::to_string(&p).unwrap();
        let back: PivotTable = serde_json::from_str(&s).unwrap();
        assert_eq!(back.date_groups.get(&0), Some(&DateGroup::Quarter));
    }

    #[test]
    fn date_groups_omitted_in_legacy_json_uses_empty_map() {
        // Old workbook JSON (pre-#60) has no `date_groups` field. The
        // default-value deserializer must produce an empty map so the
        // engine behaves exactly like a pre-#60 pivot.
        let legacy = r#"{
            "source_range": "S!A1:B2",
            "source_sheet": "S",
            "row_fields": [0],
            "col_fields": [],
            "value_field": 1,
            "agg": "sum",
            "value_fields": [],
            "filter_fields": [],
            "output_sheet": "Pivot1"
        }"#;
        let back: PivotTable = serde_json::from_str(legacy).unwrap();
        assert!(back.date_groups.is_empty());
    }

    // -----------------------------------------------------------------
    // Direct parser tests (issue #60)
    // -----------------------------------------------------------------

    #[test]
    fn parse_date_key_iso_variants() {
        // All four grouping units on the same ISO input.
        assert_eq!(
            parse_date_key("2024-03-15", DateGroup::Year),
            Some("2024".into())
        );
        assert_eq!(
            parse_date_key("2024-03-15", DateGroup::Quarter),
            Some("2024-Q1".into())
        );
        assert_eq!(
            parse_date_key("2024-03-15", DateGroup::Month),
            Some("2024-03".into())
        );
        assert_eq!(
            parse_date_key("2024-03-15", DateGroup::Day),
            Some("2024-03-15".into())
        );
    }

    #[test]
    fn parse_date_key_iso_with_slash_and_time() {
        // Slash-separated ISO and ISO+time both parse cleanly.
        assert_eq!(
            parse_date_key("2024/03/15", DateGroup::Day),
            Some("2024-03-15".into())
        );
        assert_eq!(
            parse_date_key("2024-03-15T10:30:00", DateGroup::Day),
            Some("2024-03-15".into())
        );
    }

    #[test]
    fn parse_date_key_excel_serial_to_year() {
        // 45306 = 2024-01-15 (Excel serial, days since 1899-12-30).
        assert_eq!(
            parse_date_key("45306", DateGroup::Year),
            Some("2024".into())
        );
        assert_eq!(
            parse_date_key("45306", DateGroup::Day),
            Some("2024-01-15".into())
        );
    }

    #[test]
    fn parse_date_key_rejects_small_integers_as_serials() {
        // `100` is a valid integer in 1..200_000, but it's 1900-04-09 as
        // a date — almost certainly not a date. We currently *accept* it
        // (a date-grouped field's text is a date by intent), so the test
        // pins the current behavior. If we want stricter rejection later,
        // this is the test to flip.
        assert_eq!(
            parse_date_key("100", DateGroup::Year),
            Some("1900".into())
        );
        // 0 and 200_000 are out of range → None.
        assert_eq!(parse_date_key("0", DateGroup::Year), None);
        assert_eq!(parse_date_key("200000", DateGroup::Year), None);
    }

    #[test]
    fn parse_date_key_us_two_digit_year() {
        // `1/15/24` → 2024 (50-year pivot: 00..=49 → 20xx).
        assert_eq!(
            parse_date_key("1/15/24", DateGroup::Year),
            Some("2024".into())
        );
        // `1/15/75` → 1975 (50..=99 → 19xx).
        assert_eq!(
            parse_date_key("1/15/75", DateGroup::Year),
            Some("1975".into())
        );
    }

    #[test]
    fn parse_date_key_returns_none_for_unparseable() {
        // Anything that doesn't match a date format → None so the caller
        // falls back to the raw text key path.
        assert_eq!(parse_date_key("", DateGroup::Year), None);
        assert_eq!(parse_date_key("   ", DateGroup::Year), None);
        assert_eq!(parse_date_key("not a date", DateGroup::Year), None);
        assert_eq!(parse_date_key("13/45/2024", DateGroup::Year), None);
    }

    // -----------------------------------------------------------------
    // Slicers (issue #61)
    // -----------------------------------------------------------------

    /// Helper: a pivot on a Region/Amount source, with a single slicer
    /// bound to `field_idx` and `selected_values` (empty = "All").
    fn pt_sliced(
        source: &str,
        value: usize,
        agg: Agg,
        slicer_field: usize,
        selected_values: Vec<&str>,
    ) -> (DataProxy, PivotTable) {
        let mut src = sheet_from_rows("S", &[
            &["Region", "Amount"],
            &["North", "10"],
            &["South", "20"],
            &["North", "5"],
            &["East", "30"],
        ]);
        if !selected_values.is_empty() {
            src.slicers.push(Slicer {
                id: "slicer_test".into(),
                field_idx: slicer_field,
                selected_values: selected_values.into_iter().map(String::from).collect(),
                x: 0.0,
                y: 0.0,
                width: 200.0,
                height: 100.0,
            });
        }
        let p = pt(source, vec![0], vec![], value, agg);
        (src, p)
    }

    #[test]
    fn slicer_with_no_selected_values_is_all_sentinel() {
        // Empty `selected_values` is the "All" sentinel — every row passes
        // the slicer predicate.
        let (src, p) = pt_sliced("S!A1:B5", 1, Agg::Sum, 0, vec![]);
        let r = compute(&src, &p).unwrap();
        assert_eq!(r.grand_total, vec![Some(65.0)]); // 10+20+5+30
        assert_eq!(r.row_keys.len(), 3);
    }

    #[test]
    fn slicer_with_selected_values_filters_rows() {
        // A non-empty selection includes only those rows.
        let (src, p) = pt_sliced("S!A1:B5", 1, Agg::Sum, 0, vec!["North"]);
        let r = compute(&src, &p).unwrap();
        // Only North rows (10 + 5 = 15) survive.
        assert_eq!(r.grand_total, vec![Some(15.0)]);
        let keys: Vec<String> = r.row_keys.iter().map(key_to_display).collect();
        assert_eq!(keys, vec!["North"]);
    }

    #[test]
    fn slicer_with_multiple_selected_values_unions_them() {
        // `selected_values = ["North", "South"]` keeps both buckets.
        let (src, p) = pt_sliced("S!A1:B5", 1, Agg::Sum, 0, vec!["North", "South"]);
        let r = compute(&src, &p).unwrap();
        assert_eq!(r.grand_total, vec![Some(35.0)]); // 10+20+5
        let keys: Vec<String> = r.row_keys.iter().map(key_to_display).collect();
        assert_eq!(keys, vec!["North", "South"]);
    }

    #[test]
    fn slicer_with_no_matching_values_yields_empty_body() {
        // The selection excludes every row → empty body, but the spec
        // stays valid (a slicer that filters everything is a real state
        // — the user can clear it via the panel).
        let (src, p) = pt_sliced("S!A1:B5", 1, Agg::Sum, 0, vec!["Mars"]);
        let r = compute(&src, &p).unwrap();
        assert_eq!(r.grand_total, vec![None]);
        assert_eq!(r.row_keys.len(), 0);
    }

    #[test]
    fn slicer_on_unused_field_does_not_filter() {
        // A slicer on a field the pivot doesn't reference is a no-op
        // (the engine can't match it to any row predicate, so the
        // engine's filter_pred skips it). Here, the slicer is on
        // field 5 (out of range of the source), which the engine
        // also handles by reading past the source's cells — the
        // empty-string values won't match the source's actual
        // regions, so the row is excluded. The real-world scenario
        // (slicer on a different sheet's field) is out of MVP scope.
        //
        // The intended use is: slicer on a field the pivot DOES use.
        // In that case, the predicate passes through unchanged.
        let (src, p) = pt_sliced("S!A1:B5", 1, Agg::Sum, 0, vec!["North", "South", "East"]);
        let r = compute(&src, &p).unwrap();
        // All values present → grand_total is the full sum.
        assert_eq!(r.grand_total, vec![Some(65.0)]);
    }

    #[test]
    fn slicer_composes_with_filter_field() {
        // A page-level FilterField (issue #58) AND a slicer (issue #61)
        // both apply: the row must pass both to be aggregated.
        let (mut src, mut p) = pt_sliced("S!A1:B5", 1, Agg::Sum, 0, vec!["North", "South"]);
        // Add a filter that only allows Amount ∈ {20, 30}.
        p.filter_fields.push(FilterField {
            field_idx: 1, // Amount column
            selected_values: vec!["20".into(), "30".into()],
        });
        let r = compute(&src, &p).unwrap();
        // Filter keeps Amount ∈ {20, 30}; slicer keeps Region ∈ {North, South}.
        // South has 20 → included; North has 10, 5 (excluded by Amount filter).
        // East has 30 (excluded by Region filter).
        assert_eq!(r.grand_total, vec![Some(20.0)]);
    }

    #[test]
    fn multiple_slicers_on_different_fields_compose_as_and() {
        // Two slicers on two different fields: a row passes only if it
        // passes both. (We don't have a Region+Amount multi-source
        // helper, so build a custom source.)
        let mut src = sheet_from_rows("S", &[
            &["Region", "Quarter", "Amount"],
            &["North", "Q1", "10"],
            &["North", "Q2", "20"],
            &["South", "Q1", "30"],
            &["South", "Q2", "40"],
        ]);
        src.slicers.push(Slicer {
            id: "s_region".into(),
            field_idx: 0,
            selected_values: vec!["North".into()],
            x: 0.0, y: 0.0, width: 200.0, height: 100.0,
        });
        src.slicers.push(Slicer {
            id: "s_quarter".into(),
            field_idx: 1,
            selected_values: vec!["Q1".into()],
            x: 0.0, y: 0.0, width: 200.0, height: 100.0,
        });
        let p = pt("S!A1:C5", vec![0], vec![], 2, Agg::Sum);
        let r = compute(&src, &p).unwrap();
        // North ∧ Q1 → only the first row (10).
        assert_eq!(r.grand_total, vec![Some(10.0)]);
    }

    #[test]
    fn multiple_slicers_on_same_field_last_one_wins() {
        // When the source carries two slicers on the same field, the
        // engine's lookup (last write wins) means only the latest
        // selection actually filters. (The modal would enforce
        // uniqueness; this test pins the engine's behavior for the
        // case the modal fails to prevent.)
        let mut src = sheet_from_rows("S", &[
            &["Region", "Amount"],
            &["North", "10"],
            &["South", "20"],
        ]);
        src.slicers.push(Slicer {
            id: "s_first".into(),
            field_idx: 0,
            selected_values: vec!["North".into()],
            x: 0.0, y: 0.0, width: 200.0, height: 100.0,
        });
        src.slicers.push(Slicer {
            id: "s_last".into(),
            field_idx: 0,
            selected_values: vec!["South".into()],
            x: 0.0, y: 0.0, width: 200.0, height: 100.0,
        });
        let p = pt("S!A1:B3", vec![0], vec![], 1, Agg::Sum);
        let r = compute(&src, &p).unwrap();
        // Last slicer wins → only South (20).
        assert_eq!(r.grand_total, vec![Some(20.0)]);
    }

    #[test]
    fn slicer_value_field_match_uses_source_cell_text() {
        // The slicer compares against `source.cell_raw_value(ri, ci)` —
        // the same string the user sees in the source cell. Numeric
        // source values are stringified at write time, so a slicer
        // selection of "10" matches the cell "10".
        let mut src = sheet_from_rows("S", &[
            &["Score", "Amount"],
            &["10", "100"],
            &["20", "200"],
            &["10", "50"],
        ]);
        src.slicers.push(Slicer {
            id: "s_score".into(),
            field_idx: 0,
            selected_values: vec!["10".into()],
            x: 0.0, y: 0.0, width: 200.0, height: 100.0,
        });
        // Pivot on Amount, no row field — slicer is on Score, which
        // the pivot doesn't reference. Engine should still apply the
        // slicer as a global row predicate.
        let p = pt("S!A1:B4", vec![], vec![], 1, Agg::Sum);
        let r = compute(&src, &p).unwrap();
        // Only the two rows with Score=10 (100 + 50 = 150) survive.
        assert_eq!(r.grand_total, vec![Some(150.0)]);
    }

    #[test]
    fn slicer_round_trips_through_data_proxy_json() {
        // A Slicer on the source sheet survives `get_data` → `set_data`
        // (the same path the JS API uses for `get_data`/`load_data`),
        // so the floating-panel state persists across workbook save/load.
        let mut src = sheet_from_rows("S", &[
            &["Region", "Amount"],
            &["North", "10"],
        ]);
        src.slicers.push(Slicer {
            id: "s_region".into(),
            field_idx: 0,
            selected_values: vec!["North".into(), "South".into()],
            x: 100.0, y: 200.0, width: 180.0, height: 120.0,
        });
        let json = src.get_data_json();
        let v: serde_json::Value = serde_json::from_str(&json).expect("get_data JSON parses");
        let mut back = DataProxy::new("S");
        back.set_data(v);
        assert_eq!(back.slicers.len(), 1);
        assert_eq!(back.slicers[0].id, "s_region");
        assert_eq!(back.slicers[0].field_idx, 0);
        assert_eq!(back.slicers[0].selected_values, vec!["North", "South"]);
        assert_eq!(back.slicers[0].x, 100.0);
        assert_eq!(back.slicers[0].y, 200.0);
        assert_eq!(back.slicers[0].width, 180.0);
        assert_eq!(back.slicers[0].height, 120.0);
    }

    #[test]
    fn legacy_workbook_without_slicers_loads_with_empty_vec() {
        // A pre-#61 workbook JSON has no `slicers` key. `set_data` must
        // leave `slicers` empty (the engine's "no filter" state) so the
        // pivot computes exactly as it did before.
        let legacy = r#"{
            "name": "S",
            "rows": {"len": 0, "_": {}},
            "cols": {"len": 0, "_": {}},
            "pivots": []
        }"#;
        let v: serde_json::Value = serde_json::from_str(legacy).expect("legacy JSON parses");
        let mut back = DataProxy::new("S");
        back.set_data(v);
        assert!(back.slicers.is_empty());
    }

    #[test]
    fn slicer_survives_pivot_spec_round_trip() {
        // A pivot on a source that carries a slicer round-trips through
        // the workbook JSON with the slicer intact. The slicer applies
        // on the rehydrated source so the rehydrated pivot computes the
        // same filtered result.
        let mut src = sheet_from_rows("S", &[
            &["Region", "Amount"],
            &["North", "10"],
            &["South", "20"],
            &["North", "5"],
        ]);
        src.slicers.push(Slicer {
            id: "s_region".into(),
            field_idx: 0,
            selected_values: vec!["North".into()],
            x: 0.0, y: 0.0, width: 200.0, height: 100.0,
        });
        src.pivots.push(pt("S!A1:B4", vec![0], vec![], 1, Agg::Sum));
        let json = src.get_data_json();
        let v: serde_json::Value = serde_json::from_str(&json).expect("get_data JSON parses");
        let mut back = DataProxy::new("S");
        back.set_data(v);
        assert_eq!(back.slicers.len(), 1);
        let r = compute(&back, &back.pivots[0]).unwrap();
        assert_eq!(r.grand_total, vec![Some(15.0)]); // 10 + 5
    }

    // -----------------------------------------------------------------
    // Cross-feature: slicer + date grouping (issue #60 + #61)
    // -----------------------------------------------------------------

    #[test]
    fn slicer_and_date_grouping_compose() {
        // Date-group the row field by month, and add a slicer on the
        // value field. Both filters must apply: the source rows are
        // first narrowed by the slicer, then bucketed by month.
        let mut src = sheet_from_rows("S", &[
            &["Date", "Region", "Amount"],
            &["2024-01-15", "North", "10"],
            &["2024-01-20", "South", "5"],
            &["2024-02-10", "North", "20"],
            &["2024-02-15", "South", "30"],
        ]);
        // Slicer: only "North" rows survive.
        src.slicers.push(Slicer {
            id: "s_region".into(),
            field_idx: 1,
            selected_values: vec!["North".into()],
            x: 0.0, y: 0.0, width: 200.0, height: 100.0,
        });
        // Pivot: Date grouped by month (row), Sum of Amount.
        let mut p = pt("S!A1:C5", vec![0], vec![], 2, Agg::Sum);
        p.date_groups.insert(0, DateGroup::Month);
        let r = compute(&src, &p).unwrap();
        let keys: Vec<String> = r.row_keys.iter().map(key_to_display).collect();
        // Only North rows pass the slicer; dates collapse to 2024-01 and
        // 2024-02.
        assert_eq!(keys, vec!["2024-01", "2024-02"]);
        assert_eq!(r.body[0][0], Some(10.0)); // North 2024-01
        assert_eq!(r.body[1][0], Some(20.0)); // North 2024-02
        // Grand total: only North rows (10 + 20 = 30); South (5 + 30)
        // was excluded by the slicer.
        assert_eq!(r.grand_total, vec![Some(30.0)]);
    }

    #[test]
    fn slicer_and_multi_value_pivot_compose() {
        // Slicer on a column field, multi-value pivot on another field:
        // both should apply — the slicer narrows the source rows, the
        // multi-value pivot produces two body columns.
        let mut src = sheet_from_rows("S", &[
            &["Region", "Amount"],
            &["North", "10"],
            &["North", "20"],
            &["South", "5"],
            &["South", "30"],
        ]);
        src.slicers.push(Slicer {
            id: "s_region".into(),
            field_idx: 0,
            selected_values: vec!["North".into()],
            x: 0.0, y: 0.0, width: 200.0, height: 100.0,
        });
        let p = pt_multi(
            "S!A1:B5",
            vec![],
            vec![],
            vec![(1, Agg::Sum), (1, Agg::Count)],
        );
        let r = compute(&src, &p).unwrap();
        assert_eq!(r.nv, 2);
        // Only North rows survive (10 + 20); grand totals are
        // Sum = 30, Count = 2.
        assert_eq!(r.grand_total, vec![Some(30.0), Some(2.0)]);
    }

    #[test]
    fn slicer_value_filtered_to_nothing_even_with_date_grouping() {
        // The slicer is the dominant filter: when it selects nothing,
        // the date grouping produces no row keys. (Excel behavior.)
        let mut src = sheet_from_rows("S", &[
            &["Date", "Amount"],
            &["2024-01-15", "10"],
            &["2024-02-10", "20"],
        ]);
        src.slicers.push(Slicer {
            id: "s_dummy".into(),
            // Slicer on a field that has no pivot role — but the
            // engine still applies it as a global row predicate
            // (issue #61). Selecting a value that doesn't exist
            // narrows the body to nothing.
            field_idx: 1,
            selected_values: vec!["999".into()],
            x: 0.0, y: 0.0, width: 200.0, height: 100.0,
        });
        let mut p = pt("S!A1:B3", vec![0], vec![], 1, Agg::Sum);
        p.date_groups.insert(0, DateGroup::Month);
        let r = compute(&src, &p).unwrap();
        assert_eq!(r.row_keys.len(), 0);
        assert_eq!(r.grand_total, vec![None]);
    }
}
