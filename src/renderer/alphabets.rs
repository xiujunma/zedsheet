#![allow(dead_code)]

const ALPHABETS: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";

fn alphabet(index: usize) -> char {
    ALPHABETS.chars().nth(index).unwrap()
}

/// Column index → A1 letters (bijective base-26): 0→"A", 25→"Z", 26→"AA".
pub fn string_at(index: usize) -> String {
    let n = ALPHABETS.len() as isize;
    let mut i = index as isize;
    let mut vec: Vec<char> = Vec::new();
    while i >= 0 {
        vec.push(alphabet((i % n) as usize));
        i = i / n - 1;
    }
    String::from_iter(vec.into_iter().rev())
}

/// A1 letters → column index (bijective base-26): "A"→0, "Z"→25, "AA"→26.
///
/// Saturating: a column string of ≥14 ASCII letters passes the shape
/// validators but overflows isize arithmetic — saturate instead of
/// panicking under overflow-checks (the index is nonsense either way;
/// it just misses every real cell).
pub fn index_at(str: &str) -> usize {
    let n = ALPHABETS.len() as isize;
    let mut index: isize = 0;
    for c in str.chars() {
        if let Some(pos) = ALPHABETS.find(c.to_ascii_uppercase()) {
            index = index.saturating_mul(n).saturating_add(pos as isize + 1);
        }
    }
    (index - 1).max(0) as usize
}

/// Parse an A1-style cell reference into `(col, row)`, both 0-based.
///
/// Returns `None` for anything without a valid 1-based row: no digits
/// (`"A"`, `""`), row 0 (`"A0"`), or a row number that overflows `usize`
/// (`"A99999999999999999999"` — 20 digits passes every shape validator, so
/// callers must not assume shape-validated input is safe). Callers reach
/// this with host-supplied JSON and user-typed formulas, so it must never
/// panic (a panic aborts the whole WASM module).
pub fn exp2xy(expr: &str) -> Option<(usize, usize)> {
    let mut x_vec: Vec<char> = Vec::new();
    let mut y_vec: Vec<char> = Vec::new();

    for c in expr.chars() {
        if c.is_ascii_digit() {
            y_vec.push(c);
        } else {
            let uc = c.to_uppercase().next().unwrap();
            x_vec.push(uc);
        }
    }

    let x = index_at(&String::from_iter(x_vec));
    let y = String::from_iter(y_vec).parse::<usize>().ok()?;
    if y == 0 {
        return None;
    }
    Some((x, y - 1))
}

pub fn xy2expr(x: usize, y: usize) -> String {
    format!("{}{}", string_at(x), y + 1)
}

pub fn expr2expr(expr: &str, xn: usize, yn: usize) -> String {
    match exp2xy(expr) {
        Some((x, y)) => xy2expr(x + xn, y + yn),
        None => expr.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Ported from x-spreadsheet test/core/alphabet_test.js
    #[test]
    fn index_at_values() {
        assert_eq!(index_at("A"), 0);
        assert_eq!(index_at("Z"), 25);
        assert_eq!(index_at("AA"), 26);
        assert_eq!(index_at("BA"), 52);
        assert_eq!(index_at("BC"), 54);
        assert_eq!(index_at("CA"), 78);
        assert_eq!(index_at("ZA"), 26 * 26);
        assert_eq!(index_at("AAA"), 26 * 26 + 26);
    }

    #[test]
    fn string_at_values() {
        assert_eq!(string_at(0), "A");
        assert_eq!(string_at(25), "Z");
        assert_eq!(string_at(26), "AA");
        assert_eq!(string_at(54), "BC");
        assert_eq!(string_at(78), "CA");
        assert_eq!(string_at(26 * 26), "ZA");
        assert_eq!(string_at(26 * 26 + 1), "ZB");
        assert_eq!(string_at(26 * 26 + 26), "AAA");
    }

    #[test]
    fn expr2xy_a1() {
        assert_eq!(exp2xy("A1"), Some((0, 0)));
    }

    #[test]
    fn exp2xy_rejects_missing_row() {
        assert_eq!(exp2xy("A"), None);
        assert_eq!(exp2xy(""), None);
        assert_eq!(exp2xy("ZZZ"), None);
    }

    #[test]
    fn exp2xy_rejects_row_zero() {
        assert_eq!(exp2xy("A0"), None);
    }

    #[test]
    fn exp2xy_rejects_row_overflow() {
        // 20 digits passes `looks_like_cell_ref` shape validation but
        // overflows usize — previously a panic.
        assert_eq!(exp2xy("A99999999999999999999"), None);
    }

    #[test]
    fn expr2expr_offsets() {
        assert_eq!(expr2expr("A1", 1, 1), "B2");
        assert_eq!(expr2expr("A1", 2, 3), "C4");
    }

    // round-trip: index_at and string_at are inverses
    #[test]
    fn round_trip() {
        for i in [0usize, 1, 25, 26, 27, 51, 52, 701, 702, 999] {
            assert_eq!(index_at(&string_at(i)), i);
        }
    }
}
