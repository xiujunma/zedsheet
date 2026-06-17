#![allow(dead_code)]

const ALPHABETS: &'static str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";

fn alphabet(index: usize) -> char {
    return ALPHABETS.chars().nth(index).unwrap();
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
    return String::from_iter(vec.into_iter().rev());
}

/// A1 letters → column index (bijective base-26): "A"→0, "Z"→25, "AA"→26.
pub fn index_at(str: &str) -> usize {
    let n = ALPHABETS.len() as isize;
    let mut index: isize = 0;
    for c in str.chars() {
        if let Some(pos) = ALPHABETS.find(c.to_ascii_uppercase()) {
            index = index * n + (pos as isize + 1);
        }
    }
    (index - 1).max(0) as usize
}

pub fn exp2xy(expr: &str) -> (usize, usize) {
    let mut x_vec: Vec<char> = Vec::new();
    let mut y_vec: Vec<char> = Vec::new();

    for c in expr.chars() {
        if c.is_digit(10) {
            y_vec.push(c);
        } else {
            let uc = c.to_uppercase().next().unwrap();
            x_vec.push(uc);
        }
    }

    let x = index_at(&String::from_iter(x_vec.into_iter()));
    let y = String::from_iter(y_vec.into_iter())
        .parse::<usize>()
        .unwrap();
    return (x, y - 1);
}

pub fn xy2expr(x: usize, y: usize) -> String {
    return format!("{}{}", string_at(x), y + 1);
}

pub fn expr2expr(expr: &str, xn: usize, yn: usize) -> String {
    let (x, y) = exp2xy(expr);
    return xy2expr(x + xn, y + yn);
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
        assert_eq!(exp2xy("A1"), (0, 0));
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
