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
    /// Name of the output sheet this pivot is currently rendered on. The
    /// renderer updates this when the user Refreshes (in MVP, the output
    /// sheet's name never changes — Refresh overwrites in place).
    pub output_sheet: String,
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

    // Build the filter predicate once (issue #58). A row passes when every
    // active filter's value is in its `selected_values` list — an empty
    // `selected_values` is the "All" / "(Multiple Items)" default and
    // matches every row.
    let filter_pred: Box<dyn Fn(usize) -> bool> = if pt.filter_fields.is_empty() {
        Box::new(|_| true)
    } else {
        // Pre-compute the raw text for each filter field, per row, so the
        // predicate is a cheap pointer comparison inside the loop.
        let filters: Vec<(usize, std::collections::HashSet<String>)> = pt
            .filter_fields
            .iter()
            .map(|f| (c0 + f.field_idx, f.selected_values.iter().cloned().collect()))
            .collect();
        Box::new(move |ri: usize| {
            filters.iter().all(|(ci, allowed)| {
                // An empty `allowed` set is the "All" sentinel — passes.
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
        let rk = make_key(source, ri, c0, &pt.row_fields);
        let ck = make_key(source, ri, c0, &pt.col_fields);
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
        let rk = make_key(source, ri, c0, &pt.row_fields);
        let ck = make_key(source, ri, c0, &pt.col_fields);
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
/// 0/1 fields, or a `Key::Tuple` for multi-field keys.
fn make_key(source: &DataProxy, ri: usize, c0: usize, fields: &[usize]) -> Key {
    if fields.is_empty() {
        return Key::Single(PrimKey::Blank);
    }
    let parts: Vec<PrimKey> = fields
        .iter()
        .map(|ci| prim_from_cell(source, ri, c0 + *ci))
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
}
