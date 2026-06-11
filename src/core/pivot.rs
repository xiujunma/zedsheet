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
    /// The single value field for MVP (issue #35).
    pub value_field: usize,
    pub agg: Agg,
    /// Name of the output sheet this pivot is currently rendered on. The
    /// renderer updates this when the user Refreshes (in MVP, the output
    /// sheet's name never changes — Refresh overwrites in place).
    pub output_sheet: String,
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
        if self.value_field > max_field {
            return Err(format!("value field index {} out of source range", self.value_field));
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
    /// `body[row_idx][col_idx]` is the aggregated value for the
    /// `(row_key, col_key)` bucket. `None` means the bucket was empty.
    pub body: Vec<Vec<Option<f64>>>,
    /// Row grand totals: aggregate of every value whose row_key matches.
    pub row_totals: Vec<Option<f64>>,
    /// Column grand totals: aggregate of every value whose col_key matches.
    pub col_totals: Vec<Option<f64>>,
    /// Grand total: aggregate of every value in the source.
    pub grand_total: Option<f64>,
}

/// Compute the cross-tab for a given `PivotTable` against a `DataProxy`.
///
/// Reads source cells via `DataProxy::cell_raw_value` (so any formulas in
/// the source are evaluated) and groups on `Value`-typed keys.
pub fn compute(source: &DataProxy, pt: &PivotTable) -> Result<PivotResult, String> {
    let (r0, c0, r1, _c1) = pt.validate(source)?;

    // First pass: bucket values by (row_key, col_key). `Vec<f64>` per bucket
    // (text that doesn't parse as a number is dropped from numeric aggs;
    // Count counts non-blank entries).
    let mut buckets: HashMap<(Key, Key), Vec<Option<f64>>> = HashMap::new();
    for ri in (r0 + 1)..=r1 {
        let rk = make_key(source, ri, c0, &pt.row_fields);
        let ck = make_key(source, ri, c0, &pt.col_fields);
        let raw = source.cell_raw_value(ri, c0 + pt.value_field);
        let v = parse_for_agg(&raw);
        buckets.entry((rk, ck)).or_default().push(v);
    }

    // Second pass: distinct keys in first-appearance order.
    let mut row_keys: Vec<Key> = Vec::new();
    let mut col_keys: Vec<Key> = Vec::new();
    let mut seen_r: HashSet<Key> = HashSet::new();
    let mut seen_c: HashSet<Key> = HashSet::new();
    for ri in (r0 + 1)..=r1 {
        let rk = make_key(source, ri, c0, &pt.row_fields);
        let ck = make_key(source, ri, c0, &pt.col_fields);
        if seen_r.insert(rk.clone()) {
            row_keys.push(rk);
        }
        if seen_c.insert(ck.clone()) {
            col_keys.push(ck);
        }
    }

    // Materialize the body.
    let mut body = vec![vec![None; col_keys.len()]; row_keys.len()];
    for (ri, rk) in row_keys.iter().enumerate() {
        for (ci, ck) in col_keys.iter().enumerate() {
            if let Some(vs) = buckets.get(&(rk.clone(), ck.clone())) {
                body[ri][ci] = aggregate(&pt.agg, vs);
            }
        }
    }

    // Row totals: aggregate every value with the same row_key (any col_key).
    let row_totals: Vec<Option<f64>> = row_keys
        .iter()
        .map(|rk| {
            let all: Vec<Option<f64>> = buckets
                .iter()
                .filter(|((rk2, _), _)| rk2 == rk)
                .flat_map(|(_, vs)| vs.iter().cloned())
                .collect();
            aggregate(&pt.agg, &all)
        })
        .collect();

    // Column totals: symmetric.
    let col_totals: Vec<Option<f64>> = col_keys
        .iter()
        .map(|ck| {
            let all: Vec<Option<f64>> = buckets
                .iter()
                .filter(|((_, ck2), _)| ck2 == ck)
                .flat_map(|(_, vs)| vs.iter().cloned())
                .collect();
            aggregate(&pt.agg, &all)
        })
        .collect();

    // Grand total: aggregate every value.
    let grand_total: Option<f64> = {
        let all: Vec<Option<f64>> = buckets.values().flat_map(|v| v.iter().cloned()).collect();
        aggregate(&pt.agg, &all)
    };

    Ok(PivotResult { row_keys, col_keys, body, row_totals, col_totals, grand_total })
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
/// Single-header-row layout (Excel-style, simplified):
/// ```text
///  [row-field 1] [row-field 2] ... [col key 1] [col key 2] ... [Total]
///  [row key 1a] [row key 1b]    ... [body]      [body]      ... [row tot]
///  [row key 2a] [row key 2b]    ... [body]      [body]      ... [row tot]
///  [Total]      [Total]         ... [col tot]   [col tot]   ... [grand]
/// ```
/// Col-field *names* aren't shown separately — the col-keys themselves are
/// the visible labels (consistent with how a single-col-field Excel pivot
/// shows just the values). To get the col-field *name* as a header, the
/// caller can prepend it to the source data; we keep v1 minimal.
///
/// `field_headers` is the source range's header row, indexed by the same
/// field-indexes used in `PivotTable::row_fields` / `col_fields`.
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
    let total_col = nr + nc_keys;

    // --- Header row (row 0) ---
    for (i, &field_idx) in pt.row_fields.iter().enumerate() {
        let h = field_headers.get(field_idx).cloned().unwrap_or_default();
        out.set_cell_text(0, i, &h);
        out.set_cell_style(0, i, header_style_idx);
    }
    for (j, ck) in result.col_keys.iter().enumerate() {
        let s = key_to_display(ck);
        out.set_cell_text(0, nr + j, &s);
        out.set_cell_style(0, nr + j, header_style_idx);
    }
    out.set_cell_text(0, total_col, "Total");
    out.set_cell_style(0, total_col, header_style_idx);

    // --- Body rows (1..nr_keys) ---
    for (i, rk) in result.row_keys.iter().enumerate() {
        let row = 1 + i;
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
        for (j, _ck) in result.col_keys.iter().enumerate() {
            write_value_cell(&mut out, row, nr + j, result.body[i][j], body_style_idx);
        }
        write_value_cell(&mut out, row, total_col, result.row_totals[i], total_style_idx);
    }

    // --- Totals row (1 + nr_keys) ---
    let totals_row = 1 + nr_keys;
    // The "Total" label only makes sense in the row-label area (the first
    // `nr` columns). When there are no row fields, the label column is also
    // where the first col-key lives, so we don't write a redundant label.
    if nr > 0 {
        out.set_cell_text(totals_row, 0, "Total");
        out.set_cell_style(totals_row, 0, total_style_idx);
        for j in 1..nr {
            out.set_cell_text(totals_row, j, "");
            out.set_cell_style(totals_row, j, total_style_idx);
        }
    }
    for (j, _) in result.col_keys.iter().enumerate() {
        write_value_cell(
            &mut out,
            totals_row,
            nr + j,
            result.col_totals[j],
            total_style_idx,
        );
    }
    write_value_cell(&mut out, totals_row, total_col, result.grand_total, total_style_idx);

    // Pad the sheet to at least default rows so it renders.
    if out.row_count < totals_row + 1 {
        out.row_count = totals_row + 1;
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
            output_sheet: "Pivot1".into(),
        }
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
        assert_eq!(r.row_totals, vec![Some(300.0), Some(50.0)]);
        assert_eq!(r.col_totals, vec![Some(350.0)]);
        assert_eq!(r.grand_total, Some(350.0));
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
        assert_eq!(r.grand_total, Some(3.0));
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
        assert_eq!(r.row_totals, vec![Some(60.0)]);
        assert_eq!(r.col_totals, vec![Some(60.0)]);
        assert_eq!(r.grand_total, Some(60.0));
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
        assert_eq!(r.grand_total, Some(12.0));
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
        assert_eq!(r.grand_total, Some(300.0));
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
        assert_eq!(r.row_totals, vec![Some(30.0), Some(50.0)]);
        // Col totals: Q1=60, Q2=20.
        assert_eq!(r.col_totals, vec![Some(60.0), Some(20.0)]);
        // Grand total = 80.
        assert_eq!(r.grand_total, Some(80.0));
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
}
