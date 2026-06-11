//! Formula autocomplete (issue #26): function-name suggestions and argument
//! signature hints for the formula bar.
//!
//! The token-detection logic here is pure (no DOM) so it can be unit-tested;
//! `formula_bar.rs` wires it to the input element and renders the popover.

/// Implemented functions and their argument signatures. The dispatch table in
/// `core/data_proxy.rs` is a `match` (not enumerable at runtime), so the
/// catalog is maintained here. Keep names UPPERCASE.
pub(crate) const FUNCTIONS: &[(&str, &str)] = &[
    ("SUM", "SUM(number1, [number2], …)"),
    ("AVERAGE", "AVERAGE(number1, [number2], …)"),
    ("COUNT", "COUNT(value1, [value2], …)"),
    ("COUNTA", "COUNTA(value1, [value2], …)"),
    ("COUNTBLANK", "COUNTBLANK(range)"),
    ("MAX", "MAX(number1, [number2], …)"),
    ("MIN", "MIN(number1, [number2], …)"),
    ("PRODUCT", "PRODUCT(number1, [number2], …)"),
    ("ROUND", "ROUND(number, num_digits)"),
    ("ABS", "ABS(number)"),
    ("SIGN", "SIGN(number)"),
    ("INT", "INT(number)"),
    ("MOD", "MOD(number, divisor)"),
    ("POWER", "POWER(number, power)"),
    ("SQRT", "SQRT(number)"),
    ("IF", "IF(logical_test, value_if_true, [value_if_false])"),
    ("IFS", "IFS(test1, value1, [test2, value2], …)"),
    ("IFERROR", "IFERROR(value, value_if_error)"),
    ("IFNA", "IFNA(value, value_if_na)"),
    ("AND", "AND(logical1, [logical2], …)"),
    ("OR", "OR(logical1, [logical2], …)"),
    ("NOT", "NOT(logical)"),
    ("TRUE", "TRUE()"),
    ("FALSE", "FALSE()"),
    ("CHOOSE", "CHOOSE(index_num, value1, [value2], …)"),
    ("VLOOKUP", "VLOOKUP(lookup_value, table_array, col_index, [range_lookup])"),
    ("HLOOKUP", "HLOOKUP(lookup_value, table_array, row_index, [range_lookup])"),
    ("XLOOKUP", "XLOOKUP(lookup_value, lookup_array, return_array, [if_not_found])"),
    ("LOOKUP", "LOOKUP(lookup_value, lookup_vector, [result_vector])"),
    ("INDEX", "INDEX(array, row_num, [col_num])"),
    ("MATCH", "MATCH(lookup_value, lookup_array, [match_type])"),
    ("SUMIF", "SUMIF(range, criteria, [sum_range])"),
    ("SUMIFS", "SUMIFS(sum_range, criteria_range1, criteria1, …)"),
    ("COUNTIF", "COUNTIF(range, criteria)"),
    ("COUNTIFS", "COUNTIFS(criteria_range1, criteria1, …)"),
    ("AVERAGEIF", "AVERAGEIF(range, criteria, [average_range])"),
    ("AVERAGEIFS", "AVERAGEIFS(average_range, criteria_range1, criteria1, …)"),
    ("CONCAT", "CONCAT(text1, [text2], …)"),
    ("CONCATENATE", "CONCATENATE(text1, [text2], …)"),
    ("TEXTJOIN", "TEXTJOIN(delimiter, ignore_empty, text1, …)"),
    ("LEFT", "LEFT(text, [num_chars])"),
    ("RIGHT", "RIGHT(text, [num_chars])"),
    ("MID", "MID(text, start_num, num_chars)"),
    ("LEN", "LEN(text)"),
    ("LOWER", "LOWER(text)"),
    ("UPPER", "UPPER(text)"),
    ("TRIM", "TRIM(text)"),
    ("SUBSTITUTE", "SUBSTITUTE(text, old_text, new_text, [instance])"),
    ("FIND", "FIND(find_text, within_text, [start_num])"),
    ("SEARCH", "SEARCH(find_text, within_text, [start_num])"),
    ("VALUE", "VALUE(text)"),
    ("TEXT", "TEXT(value, format_text)"),
    ("ROW", "ROW([reference])"),
    ("COLUMN", "COLUMN([reference])"),
    ("OFFSET", "OFFSET(reference, rows, cols, [height], [width])"),
    ("INDIRECT", "INDIRECT(ref_text)"),
    ("ADDRESS", "ADDRESS(row_num, col_num, [abs_num])"),
    ("DATE", "DATE(year, month, day)"),
    ("TODAY", "TODAY()"),
    ("NOW", "NOW()"),
    ("ISERROR", "ISERROR(value)"),
    ("ISNA", "ISNA(value)"),
    ("ISNUMBER", "ISNUMBER(value)"),
    ("ISTEXT", "ISTEXT(value)"),
    ("ISBLANK", "ISBLANK(value)"),
    // Dynamic-array functions — results spill into neighboring cells (issue #33).
    ("FILTER", "FILTER(array, include, [if_empty])"),
    ("SORT", "SORT(array, [sort_index], [sort_order], [by_col])"),
    ("SORTBY", "SORTBY(array, by_array1, [sort_order1], …)"),
    ("UNIQUE", "UNIQUE(array, [by_col], [exactly_once])"),
    ("SEQUENCE", "SEQUENCE(rows, [columns], [start], [step])"),
    ("RANDARRAY", "RANDARRAY([rows], [columns], [min], [max], [whole_number])"),
];

/// Characters that can precede a function name — formula start, open paren,
/// argument separator, an operator, or a space following one of those.
fn is_name_boundary(c: char) -> bool {
    matches!(
        c,
        '=' | '(' | ',' | '+' | '-' | '*' | '/' | '<' | '>' | '&' | '%' | '^' | ' '
    )
}

/// The function-name prefix being typed at `caret`, with its start byte offset
/// — `Some((start, "SU"))` for `"=SU|"`. `None` when the text isn't a formula,
/// the caret isn't on an identifier, or that identifier isn't in a
/// function-name position (so cell refs like `A1` don't trigger suggestions).
pub(crate) fn prefix_at(text: &str, caret: usize) -> Option<(usize, String)> {
    if !text.trim_start().starts_with('=') {
        return None;
    }
    let caret = caret.min(text.len());
    let head = &text[..caret];
    // Start of the trailing run of ASCII letters ending at the caret.
    let start = head
        .char_indices()
        .rev()
        .take_while(|(_, c)| c.is_ascii_alphabetic())
        .last()
        .map(|(i, _)| i)?;
    let prefix = &head[start..];
    if prefix.is_empty() {
        return None;
    }
    // The char before the run must put us at a function-name position.
    let at_boundary = head[..start]
        .chars()
        .next_back()
        .map(is_name_boundary)
        .unwrap_or(true); // nothing before (e.g. caret right after a lone "=")
    if at_boundary {
        Some((start, prefix.to_uppercase()))
    } else {
        None
    }
}

/// Catalog entries whose name starts with `prefix` (case-insensitive),
/// in catalog order. Empty prefix matches nothing.
pub(crate) fn matches(prefix: &str) -> Vec<&'static (&'static str, &'static str)> {
    if prefix.is_empty() {
        return Vec::new();
    }
    let up = prefix.to_uppercase();
    FUNCTIONS.iter().filter(|(n, _)| n.starts_with(&up)).collect()
}

/// The signature of the function whose `(` encloses `caret`, for the argument
/// hint — `Some("SUM(number1, …)")` when the caret is inside `SUM(`. Walks
/// backward tracking paren depth to find the innermost open call.
pub(crate) fn active_signature(text: &str, caret: usize) -> Option<&'static str> {
    if !text.trim_start().starts_with('=') {
        return None;
    }
    let head: Vec<char> = text[..caret.min(text.len())].chars().collect();
    let mut depth = 0i32;
    let mut i = head.len();
    while i > 0 {
        i -= 1;
        match head[i] {
            ')' => depth += 1,
            '(' if depth == 0 => {
                let mut j = i;
                while j > 0 && head[j - 1].is_ascii_alphabetic() {
                    j -= 1;
                }
                if j == i {
                    return None; // '(' not preceded by a name (grouping paren)
                }
                let name: String = head[j..i].iter().collect::<String>().to_uppercase();
                return FUNCTIONS.iter().find(|(n, _)| *n == name).map(|(_, s)| *s);
            }
            '(' => depth -= 1,
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_detects_function_position() {
        assert_eq!(prefix_at("=SU", 3), Some((1, "SU".to_string())));
        assert_eq!(prefix_at("=1+SU", 5), Some((3, "SU".to_string())));
        assert_eq!(prefix_at("=SUM(a, AV", 10), Some((8, "AV".to_string())));
        // Not a formula, or not a name position.
        assert_eq!(prefix_at("SU", 2), None); // no leading '='
        assert_eq!(prefix_at("=A1", 3), None); // cell ref (ends in digit)
        assert_eq!(prefix_at("=SUM(", 5), None); // caret after '(' — no prefix
        // Caret in the middle of a word uses the part up to the caret.
        assert_eq!(prefix_at("=SUMX", 3), Some((1, "SU".to_string())));
    }

    #[test]
    fn matches_by_prefix() {
        let m: Vec<&str> = matches("SU").iter().map(|(n, _)| *n).collect();
        assert!(m.contains(&"SUM"));
        assert!(m.contains(&"SUMIF"));
        assert!(m.contains(&"SUBSTITUTE"));
        assert!(!m.contains(&"AVERAGE"));
        assert!(matches("").is_empty());
        assert!(matches("ZZZZ").is_empty());
        // Case-insensitive.
        assert_eq!(
            matches("su").iter().map(|(n, _)| *n).collect::<Vec<_>>(),
            matches("SU").iter().map(|(n, _)| *n).collect::<Vec<_>>()
        );
    }

    #[test]
    fn signature_for_enclosing_call() {
        assert_eq!(active_signature("=SUM(", 5), Some("SUM(number1, [number2], …)"));
        assert_eq!(active_signature("=SUM(A1, ", 9), Some("SUM(number1, [number2], …)"));
        // Innermost call wins.
        assert_eq!(
            active_signature("=SUM(ROUND(", 11),
            Some("ROUND(number, num_digits)")
        );
        // A bare grouping paren has no signature.
        assert_eq!(active_signature("=(1+", 4), None);
        // Closed call: caret is outside it again.
        assert_eq!(active_signature("=SUM(A1)", 8), None);
    }
}
