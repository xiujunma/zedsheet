//! Integration tests for the PivotTable feature (issue #35).
//!
//! Covers the full model + materialization flow on the host build (no wasm):
//! build a `DataProxy` source, build a `PivotTable` spec, run `compute`,
//! materialize the result, and verify the cells land where expected. Also
//! round-trips through `DataProxy::get_data` / `set_data` to confirm the
//! `pivots` field survives the JSON wire format.

use zedsheet::core::data_proxy::DataProxy;
use zedsheet::core::pivot::{
    compute, materialize, read_field_headers, Agg, PivotTable, ValueField,
};

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
        source_sheet: "Sales".into(),
        row_fields,
        col_fields,
        value_field: value,
        agg,
        value_fields: vec![],
        filter_fields: vec![],
        date_groups: std::collections::HashMap::new(),
        output_sheet: "Pivot1".into(),
    }
}

#[test]
fn full_pivot_pipeline_writes_expected_cells() {
    // Source: a small sales table.
    let src = sheet_from_rows("Sales", &[
        &["Region", "Quarter", "Amount"],
        &["North", "Q1", "10"],
        &["North", "Q2", "20"],
        &["South", "Q1", "50"],
    ]);

    // 1) Compute the cross-tab: rows=Region, cols=Quarter, value=Amount (sum).
    let p = pt("Sales!A1:C4", vec![0], vec![1], 2, Agg::Sum);
    let r = compute(&src, &p).unwrap();
    assert_eq!(r.row_keys.len(), 2); // North, South
    assert_eq!(r.col_keys.len(), 2); // Q1, Q2
    assert_eq!(r.body[0][0], Some(10.0));
    assert_eq!(r.body[1][0], Some(50.0));
    assert_eq!(r.grand_total, vec![Some(80.0)]);

    // 2) Materialize onto a fresh read-only sheet.
    let headers = read_field_headers(&src, 0, 0, 2);
    let out = materialize(&p, &r, &headers, "Pivot1");
    assert_eq!(out.get_cell_text(0, 0), "Region");
    assert_eq!(out.get_cell_text(0, 1), "Q1");
    assert_eq!(out.get_cell_text(0, 2), "Q2");
    assert_eq!(out.get_cell_text(0, 3), "Total");
    assert_eq!(out.get_cell_text(1, 0), "North");
    assert_eq!(out.get_cell_text(1, 1), "10");
    assert_eq!(out.get_cell_text(1, 2), "20");
    assert_eq!(out.get_cell_text(1, 3), "30");
    assert_eq!(out.get_cell_text(3, 0), "Total");
    assert_eq!(out.get_cell_text(3, 1), "60");
    assert_eq!(out.get_cell_text(3, 2), "20");
    assert_eq!(out.get_cell_text(3, 3), "80");
}

#[test]
fn pivot_spec_round_trips_through_data_proxy_json() {
    // Spec persistence: a pivot on the source sheet should round-trip via
    // `DataProxy::get_data` → `set_data` (the same path the JS API uses
    // for `get_data`/`load_data`).
    let mut src = sheet_from_rows("Sales", &[
        &["Name", "Score"],
        &["Alice", "10"],
        &["Bob", "20"],
    ]);
    let p = pt("Sales!A1:B3", vec![0], vec![], 1, Agg::Sum);
    src.pivots.push(p.clone());

    let json = src.get_data_json();
    let v: serde_json::Value = serde_json::from_str(&json).expect("get_data JSON parses");
    let mut back = DataProxy::new("Sales");
    back.set_data(v);
    // The pivots list is on the source.
    assert_eq!(back.pivots.len(), 1);
    assert_eq!(back.pivots[0].row_fields, vec![0]);
    assert_eq!(back.pivots[0].value_field, 1);
    assert_eq!(back.pivots[0].agg, Agg::Sum);
    assert_eq!(back.pivots[0].output_sheet, "Pivot1");

    // Compute on the rehydrated source still works.
    let r = compute(&back, &back.pivots[0]).unwrap();
    assert_eq!(r.grand_total, vec![Some(30.0)]);
}

#[test]
fn refresh_recomputes_against_modified_source() {
    // The pivot's Refresh path re-reads the source. If the source data
    // changes between the original create and the refresh, the recomputed
    // cross-tab should reflect the new data.
    let mut src = sheet_from_rows("Sales", &[
        &["Name", "Score"],
        &["Alice", "10"],
        &["Bob", "20"],
    ]);
    // Source range A1:B3 — header + 2 data rows. Editing values inside
    // the range simulates a refresh after the user changes source data.
    let p = pt("Sales!A1:B3", vec![0], vec![], 1, Agg::Sum);
    src.pivots.push(p.clone());

    // Original cross-tab: Alice=10, Bob=20 → grand_total = 30.
    let r1 = compute(&src, &p).unwrap();
    assert_eq!(r1.grand_total, vec![Some(30.0)]);
    assert_eq!(r1.row_keys.len(), 2);

    // Mutate source: bump Alice's score to 50. Refresh should pick it up.
    src.set_cell_text(1, 1, "50");

    // Refresh: 50 + 20 = 70.
    let r2 = compute(&src, &p).unwrap();
    assert_eq!(r2.grand_total, vec![Some(70.0)]);
}

#[test]
fn spec_on_source_round_trips_through_get_data_set_data() {
    // A pivot spec on the source sheet survives a `get_data` → `set_data`
    // round-trip (the same path the JS API uses for `get_data`/`load_data`).
    // The renderer's `install_pivot_into_registry` relies on this — it
    // pushes the spec onto the source so the registry persists it; the
    // round-trip below is what proves the persistence is real.
    let mut src = sheet_from_rows("Sales", &[
        &["Name", "Score"],
        &["Alice", "10"],
    ]);
    let p = pt("Sales!A1:B2", vec![0], vec![], 1, Agg::Sum);
    src.pivots.push(p);

    let json = src.get_data_json();
    let v: serde_json::Value = serde_json::from_str(&json).expect("get_data JSON parses");
    let mut back = DataProxy::new("Sales");
    back.set_data(v);
    assert_eq!(back.pivots.len(), 1);
    assert_eq!(back.pivots[0].output_sheet, "Pivot1");
}

#[test]
fn multi_value_pivot_pipeline_writes_expected_cells() {
    // End-to-end pipeline for a multi-value pivot (issue #59):
    //   Sum of Amount  | Count of Amount
    // grouped by Region (row) × Quarter (col).
    //
    // Source:
    //   North, Q1, 10
    //   North, Q2, 20
    //   South, Q1, 50
    let src = sheet_from_rows("Sales", &[
        &["Region", "Quarter", "Amount"],
        &["North", "Q1", "10"],
        &["North", "Q2", "20"],
        &["South", "Q1", "50"],
    ]);

    // Build a PivotTable with two value fields: Sum of Amount, Count of Amount.
    let mut p = pt("Sales!A1:C4", vec![0], vec![1], 0, Agg::Sum);
    p.value_fields = vec![
        ValueField { field: 2, agg: Agg::Sum },
        ValueField { field: 2, agg: Agg::Count },
    ];

    // 1) Compute.
    let r = compute(&src, &p).unwrap();
    assert_eq!(r.nv, 2);
    assert_eq!(r.grand_total, vec![Some(80.0), Some(3.0)]);

    // 2) Materialize.
    let headers = read_field_headers(&src, 0, 0, 2);
    let out = materialize(&p, &r, &headers, "Pivot1");

    // Top header row carries the value-field names.
    assert_eq!(out.get_cell_text(0, 0), "");
    assert_eq!(out.get_cell_text(0, 1), "Sum of Amount");
    assert_eq!(out.get_cell_text(0, 4), "Count of Amount");

    // Main header row: row-key + col-keys + Total, repeated per value field.
    assert_eq!(out.get_cell_text(1, 0), "Region");
    assert_eq!(out.get_cell_text(1, 1), "Q1");
    assert_eq!(out.get_cell_text(1, 2), "Q2");
    assert_eq!(out.get_cell_text(1, 3), "Total");
    assert_eq!(out.get_cell_text(1, 4), "Q1");
    assert_eq!(out.get_cell_text(1, 5), "Q2");
    assert_eq!(out.get_cell_text(1, 6), "Total");

    // Body rows.
    assert_eq!(out.get_cell_text(2, 0), "North");
    assert_eq!(out.get_cell_text(2, 1), "10");
    assert_eq!(out.get_cell_text(2, 2), "20");
    assert_eq!(out.get_cell_text(2, 3), "30");
    assert_eq!(out.get_cell_text(2, 4), "1");
    assert_eq!(out.get_cell_text(2, 5), "1");
    assert_eq!(out.get_cell_text(2, 6), "2");

    assert_eq!(out.get_cell_text(3, 0), "South");
    assert_eq!(out.get_cell_text(3, 1), "50");
    assert_eq!(out.get_cell_text(3, 2), ""); // Q2 bucket empty for South
    assert_eq!(out.get_cell_text(3, 3), "50");
    assert_eq!(out.get_cell_text(3, 4), "1");
    assert_eq!(out.get_cell_text(3, 5), ""); // empty
    assert_eq!(out.get_cell_text(3, 6), "1");

    // Totals row.
    assert_eq!(out.get_cell_text(4, 0), "Total");
    assert_eq!(out.get_cell_text(4, 1), "60");
    assert_eq!(out.get_cell_text(4, 2), "20");
    assert_eq!(out.get_cell_text(4, 3), "80");
    assert_eq!(out.get_cell_text(4, 4), "2");
    assert_eq!(out.get_cell_text(4, 5), "1");
    assert_eq!(out.get_cell_text(4, 6), "3");
}

#[test]
fn multi_value_pivot_spec_survives_json_round_trip() {
    // The `value_fields` list must survive `get_data` / `set_data` so the
    // workbook-level persistence preserves multi-value pivots. This pins
    // the serde layout (issue #59).
    let mut src = sheet_from_rows("Sales", &[
        &["Region", "Amount"],
        &["North", "10"],
        &["South", "20"],
    ]);
    let mut p = pt("Sales!A1:B3", vec![0], vec![], 1, Agg::Sum);
    p.value_fields = vec![
        ValueField { field: 1, agg: Agg::Sum },
        ValueField { field: 1, agg: Agg::Count },
    ];
    src.pivots.push(p);

    let json = src.get_data_json();
    let v: serde_json::Value = serde_json::from_str(&json).expect("get_data JSON parses");
    let mut back = DataProxy::new("Sales");
    back.set_data(v);
    assert_eq!(back.pivots.len(), 1);
    assert_eq!(back.pivots[0].value_fields.len(), 2);
    assert_eq!(back.pivots[0].value_fields[0].field, 1);
    assert_eq!(back.pivots[0].value_fields[0].agg, Agg::Sum);
    assert_eq!(back.pivots[0].value_fields[1].field, 1);
    assert_eq!(back.pivots[0].value_fields[1].agg, Agg::Count);

    // The rehydrated multi-value pivot computes correctly.
    let r = compute(&back, &back.pivots[0]).unwrap();
    assert_eq!(r.grand_total, vec![Some(30.0), Some(2.0)]);
}

#[test]
fn legacy_workbook_without_value_fields_still_works() {
    // A workbook saved with the pre-#59 format — empty `value_fields`,
    // legacy `value_field` + `agg` set — must still load and compute
    // (issue #59, backward compat).
    let mut src = sheet_from_rows("Sales", &[
        &["Region", "Amount"],
        &["North", "10"],
        &["South", "20"],
    ]);
    // Pre-#59 shape: empty value_fields, single value_field + agg.
    let p = pt("Sales!A1:B3", vec![0], vec![], 1, Agg::Sum);
    assert!(p.value_fields.is_empty()); // sanity
    src.pivots.push(p);

    let json = src.get_data_json();
    let v: serde_json::Value = serde_json::from_str(&json).expect("get_data JSON parses");
    let mut back = DataProxy::new("Sales");
    back.set_data(v);
    assert_eq!(back.pivots.len(), 1);
    // The engine still computes a single-value result via the legacy path.
    let r = compute(&back, &back.pivots[0]).unwrap();
    assert_eq!(r.nv, 1);
    assert_eq!(r.grand_total, vec![Some(30.0)]);
}
