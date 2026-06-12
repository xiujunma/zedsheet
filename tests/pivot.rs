//! Integration tests for the PivotTable feature (issue #35).
//!
//! Covers the full model + materialization flow on the host build (no wasm):
//! build a `DataProxy` source, build a `PivotTable` spec, run `compute`,
//! materialize the result, and verify the cells land where expected. Also
//! round-trips through `DataProxy::get_data` / `set_data` to confirm the
//! `pivots` field survives the JSON wire format.

use zedsheet::core::data_proxy::DataProxy;
use zedsheet::core::pivot::{
    compute, materialize, read_field_headers, Agg, PivotTable,
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
        filter_fields: vec![],
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
    assert_eq!(r.grand_total, Some(80.0));

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
    assert_eq!(r.grand_total, Some(30.0));
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
    assert_eq!(r1.grand_total, Some(30.0));
    assert_eq!(r1.row_keys.len(), 2);

    // Mutate source: bump Alice's score to 50. Refresh should pick it up.
    src.set_cell_text(1, 1, "50");

    // Refresh: 50 + 20 = 70.
    let r2 = compute(&src, &p).unwrap();
    assert_eq!(r2.grand_total, Some(70.0));
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
