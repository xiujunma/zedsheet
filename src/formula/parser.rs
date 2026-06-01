use regex::Regex;

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Number(f64),
    CellRef(String),
    /// A named-range reference (an identifier that isn't a cell ref or function).
    Name(String),
    Range(String),
    /// A sheet-qualified single cell ref like `Sheet2!A1` (issue #4).
    SheetCellRef { sheet: String, ref_: String },
    /// A sheet-qualified range like `Sheet2!A1:B3` (issue #4).
    SheetRange { sheet: String, from: String, to: String },
    Operator(String),
    Function(String),
    LeftParen,
    RightParen,
    Comma,
    Colon,
    String(String),
    /// A literal error value like `#DIV/0!` or `#N/A`.
    Error(String),
}

pub struct FormulaParser {
    // regex for cell reference like A1, B2, AA100
    cell_ref_regex: Regex,
}

impl Default for FormulaParser {
    fn default() -> Self {
        FormulaParser {
            cell_ref_regex: Regex::new(r"^[A-Za-z]+[0-9]+$").unwrap(),
        }
    }
}

impl FormulaParser {
    pub fn new() -> Self {
        FormulaParser::default()
    }

    pub fn parse(&self, expr: &str) -> Result<Vec<Token>, String> {
        let mut tokens = Vec::new();
        let mut chars = expr.chars().peekable();
        let mut current_number = String::new();
        let mut in_string = false;
        let mut string_content = String::new();

        while let Some(c) = chars.next() {
            if in_string {
                if c == '"' {
                    tokens.push(Token::String(string_content.clone()));
                    string_content.clear();
                    in_string = false;
                } else {
                    string_content.push(c);
                }
                continue;
            }

            if c == '"' {
                in_string = true;
                continue;
            }

            if c.is_whitespace() {
                continue;
            }

            if c.is_numeric() || (c == '.' && !current_number.is_empty()) {
                current_number.push(c);
                while let Some(&next) = chars.peek() {
                    if next.is_numeric() || next == '.' {
                        current_number.push(chars.next().unwrap());
                    } else {
                        break;
                    }
                }
                tokens.push(Token::Number(current_number.parse().unwrap_or(0.0)));
                current_number.clear();
            } else if c.is_alphabetic() || c == '$' {
                let mut ident = String::new();
                ident.push(c);
                while let Some(&next) = chars.peek() {
                    if next.is_alphanumeric() || next == '$' {
                        ident.push(chars.next().unwrap());
                    } else {
                        break;
                    }
                }
                // Check if it's a function call
                if let Some(&'(') = chars.peek() {
                    tokens.push(Token::Function(ident.to_uppercase()));
                } else if self.cell_ref_regex.is_match(&ident) {
                    tokens.push(Token::CellRef(ident.to_uppercase()));
                } else {
                    return Err(format!("Unknown identifier: {}", ident));
                }
            } else {
                match c {
                    '+' | '-' | '*' | '/' | '=' | '>' | '<' => {
                        let mut op = String::from(c);
                        if let Some(&next) = chars.peek() {
                            if (c == '=' && next == '=') ||
                               (c == '>' && (next == '=' || next == '>')) ||
                               (c == '<' && (next == '=' || next == '<')) {
                                op.push(chars.next().unwrap());
                            }
                        }
                        // Handle = as comparison operator
                        if op == "=" {
                            // Could be comparison or formula marker - treat as comparison
                        }
                        tokens.push(Token::Operator(op));
                    }
                    '(' => tokens.push(Token::LeftParen),
                    ')' => tokens.push(Token::RightParen),
                    ':' => tokens.push(Token::Colon),
                    ',' => tokens.push(Token::Comma),
                    _ => {}
                }
            }
        }

        Ok(tokens)
    }
}

/// Convert infix expression to suffix expression
/// Example: "AVERAGE(SUM(A1,A2), B1) + 50 + B20"
/// Returns: tokens with operators and function markers
pub fn infix_expr_to_suffix_expr(src: &str) -> Vec<String> {
    let mut operator_stack: Vec<String> = Vec::new();
    let mut stack: Vec<String> = Vec::new();
    let mut fn_arg_type = 0; // 1 => comma, 2 => colon (range), 3 => comparison
    let mut fn_arg_operator = String::new();
    let mut fn_args_len = 1usize;
    let mut oldc = ' ';
    let mut sub_strs: Vec<char> = Vec::new();
    let mut chars = src.chars().peekable();
    let result: Vec<String> = Vec::new();

    while let Some(c) = chars.next() {
        if c == ' ' {
            continue;
        }

        if c.is_alphabetic() {
            sub_strs.push(c.to_ascii_uppercase());
        } else if c.is_numeric() || (c == '.' && !sub_strs.is_empty()) {
            sub_strs.push(c);
        } else if c == '"' {
            // String literal
            let mut s = String::new();
            loop {
                if let Some(next) = chars.next() {
                    if next == '"' {
                        break;
                    }
                    s.push(next);
                } else {
                    break;
                }
            }
            stack.push(format!("\"{}", s));
        } else if c == '-' && is_arg_start(oldc) {
            // Negative number
            sub_strs.push(c);
        } else {
            // Operator or punctuation
            if !sub_strs.is_empty() && c != '(' {
                stack.push(sub_strs.iter().collect());
            }

            if c == '(' {
                if !sub_strs.is_empty() {
                    // Function call
                    operator_stack.push(sub_strs.iter().collect());
                }
                sub_strs.clear();
            } else if c == ')' {
                // Pop from operator stack until matching '('
                let mut top = operator_stack.pop();
                while let Some(op) = top {
                    if op == "(" {
                        top = None;
                        break;
                    }
                    stack.push(op);
                    top = operator_stack.pop();
                }

                // Handle function argument types
                if fn_arg_type == 2 {
                    // Range: pop two cell refs and expand
                    if let (Some(end), Some(start)) = (stack.pop(), stack.pop()) {
                        // Range detected - push range token
                        stack.push(format!("{}:{}", start, end));
                        stack.push(vec![top.unwrap_or_default(), "2".to_string()].join(","));
                    }
                } else if fn_arg_type == 1 || fn_arg_type == 3 {
                    if fn_arg_type == 3 && !fn_arg_operator.is_empty() {
                        stack.push(fn_arg_operator.clone());
                    }
                    // Function args
                    let func_name = operator_stack.pop().unwrap_or_default();
                    stack.push(format!("[{},{}]", func_name, fn_args_len));
                    fn_args_len = 1;
                }

                fn_arg_type = 0;
            } else if c == '=' || c == '>' || c == '<' {
                let nc = chars.peek().cloned();
                fn_arg_operator = c.to_string();
                if let Some('=') = nc {
                    fn_arg_operator.push('=');
                    chars.next();
                } else if let Some('>') = nc {
                    fn_arg_operator.push('>');
                    chars.next();
                } else if let Some('<') = nc {
                    fn_arg_operator.push('<');
                    chars.next();
                }
                fn_arg_type = 3;
            } else if c == ':' {
                fn_arg_type = 2;
            } else if c == ',' {
                if fn_arg_type == 3 {
                    stack.push(fn_arg_operator.clone());
                }
                fn_arg_type = 1;
                fn_args_len += 1;
            } else if c == '+' || c == '-' || c == '*' || c == '/' {
                // Operator precedence
                while let Some(top) = operator_stack.last().cloned() {
                    if top == "(" {
                        break;
                    }
                    if (c == '+' || c == '-') && (top == "*" || top == "/") {
                        break;
                    }
                    stack.push(operator_stack.pop().unwrap());
                }
                operator_stack.push(c.to_string());
            }

            sub_strs.clear();
        }

        oldc = c;
    }

    if !sub_strs.is_empty() {
        stack.push(sub_strs.iter().collect());
    }

    while let Some(op) = operator_stack.pop() {
        stack.push(op);
    }

    result
}

fn is_arg_start(c: char) -> bool {
    matches!(c, '(') || c == ',' || c == '=' || c == '<' || c == '>'
}

// Tokenize a formula string for evaluation
/// True if `s` looks like a cell reference (`A1`, `$B$2`, `AA100`): one or more
/// letters followed by one or more digits, with optional `$` anchors. Used to
/// tell a cell reference apart from a named-range reference while tokenizing.
pub fn looks_like_cell_ref(s: &str) -> bool {
    let bytes: Vec<u8> = s.bytes().filter(|&b| b != b'$').collect();
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
        i += 1;
    }
    let letters = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    letters > 0 && i > letters && i == bytes.len()
}

/// True if `s` is a valid bare sheet name for cross-sheet refs (issue #4).
/// Excel allows quoted names with spaces/specials via `'name'!A1`; this
/// unquoted form covers identifier-style names like `Sheet`, `Sheet2`, `_q3`.
/// The presence of digits is fine — `sheet2` is a sheet, while `A1` (matched
/// first as a cell ref) never reaches the `!` path.
fn is_sheet_name(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Read a cell-ref-shaped token (e.g. `A1`, `$B$2`) starting at `*i`, advancing
/// the cursor past the consumed characters. Returns `None` if the next text
/// doesn't look like a cell ref.
fn read_cell_ref(chars: &[char], i: &mut usize) -> Option<String> {
    let mut s = String::new();
    // Optional leading '$' for column anchor.
    if *i < chars.len() && chars[*i] == '$' {
        s.push('$');
        *i += 1;
    }
    // One or more letters for the column.
    let start = s.len();
    while *i < chars.len() && chars[*i].is_ascii_alphabetic() {
        s.push(chars[*i].to_ascii_uppercase());
        *i += 1;
    }
    if s.len() == start {
        return None;
    }
    // Optional '$' for row anchor.
    if *i < chars.len() && chars[*i] == '$' {
        s.push('$');
        *i += 1;
    }
    // One or more digits for the row.
    let dstart = s.len();
    while *i < chars.len() && chars[*i].is_ascii_digit() {
        s.push(chars[*i]);
        *i += 1;
    }
    if s.len() == dstart {
        return None;
    }
    Some(s)
}

pub fn tokenize(formula: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = formula.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];

        if c.is_whitespace() {
            i += 1;
            continue;
        }

        if c.is_numeric() || (c == '.' && i + 1 < chars.len() && chars[i + 1].is_numeric()) {
            let mut num_str = String::new();
            while i < chars.len() && (chars[i].is_numeric() || chars[i] == '.') {
                num_str.push(chars[i]);
                i += 1;
            }
            if let Ok(num) = num_str.parse::<f64>() {
                tokens.push(Token::Number(num));
            }
            continue;
        }

        if c == '"' {
            let mut str_content = String::new();
            i += 1;
            while i < chars.len() && chars[i] != '"' {
                str_content.push(chars[i]);
                i += 1;
            }
            i += 1; // Skip closing quote
            tokens.push(Token::String(str_content));
            continue;
        }

        // Error literal, e.g. #DIV/0!, #N/A, #NAME?, #REF!
        if c == '#' {
            let mut lit = String::from('#');
            i += 1;
            while i < chars.len()
                && (chars[i].is_ascii_alphanumeric()
                    || matches!(chars[i], '/' | '?' | '!' | '.'))
            {
                lit.push(chars[i]);
                i += 1;
            }
            tokens.push(Token::Error(lit));
            continue;
        }

        if c.is_alphabetic() || c == '$' {
            let mut ident = String::new();
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '$') {
                ident.push(chars[i].to_ascii_uppercase());
                i += 1;
            }
            // Sheet-qualified ref like `Sheet2!A1` or `Sheet2!A1:B3` (issue #4).
            // The sheet-name part is identifier-shaped: `[A-Za-z_][A-Za-z0-9_]*`.
            // If the next char is `!`, this `ident` is the sheet name and the
            // remaining text must look like a cell ref (or a range). Otherwise
            // we fall through to the function / cell-ref / name classification.
            if i < chars.len() && chars[i] == '!' && is_sheet_name(&ident) {
                let sheet = ident.clone();
                i += 1; // consume '!'
                let first_ref = match read_cell_ref(&chars, &mut i) {
                    Some(r) => r,
                    None => {
                        // Malformed: emit a Name and let the rest fall through.
                        tokens.push(Token::Name(sheet));
                        continue;
                    }
                };
                if i < chars.len() && chars[i] == ':' {
                    i += 1; // consume ':'
                    match read_cell_ref(&chars, &mut i) {
                        Some(end) => tokens.push(Token::SheetRange { sheet, from: first_ref, to: end }),
                        None => {
                            // No closing ref — treat the whole thing as a
                            // single-cell ref and let the leftover ':' be
                            // picked up as a Colon token.
                            tokens.push(Token::SheetCellRef { sheet, ref_: first_ref });
                        }
                    }
                } else {
                    tokens.push(Token::SheetCellRef { sheet, ref_: first_ref });
                }
                continue;
            }
            if i < chars.len() && chars[i] == '(' {
                tokens.push(Token::Function(ident));
            } else if looks_like_cell_ref(&ident) {
                tokens.push(Token::CellRef(ident));
            } else {
                // An identifier that isn't a cell ref or function call is a
                // named-range reference (resolved by the evaluator).
                tokens.push(Token::Name(ident));
            }
            continue;
        }

        match c {
            '+' => { tokens.push(Token::Operator("+".to_string())); i += 1; }
            '-' => { tokens.push(Token::Operator("-".to_string())); i += 1; }
            '*' => { tokens.push(Token::Operator("*".to_string())); i += 1; }
            '/' => { tokens.push(Token::Operator("/".to_string())); i += 1; }
            '(' => { tokens.push(Token::LeftParen); i += 1; }
            ')' => { tokens.push(Token::RightParen); i += 1; }
            ':' => { tokens.push(Token::Colon); i += 1; }
            ',' => { tokens.push(Token::Comma); i += 1; }
            // Comparison operators, consuming a trailing '=' for >=/<=/==.
            '>' | '<' | '=' => {
                let mut op = c.to_string();
                if i + 1 < chars.len() && chars[i + 1] == '=' {
                    op.push('=');
                    i += 1;
                }
                tokens.push(Token::Operator(op));
                i += 1;
            }
            _ => { i += 1; }
        }
    }

    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tokenize_simple() {
        let tokens = tokenize("A1+B2");
        println!("{:?}", tokens);
    }

    #[test]
    fn test_parse() {
        let parser = FormulaParser::new();
        let result = parser.parse("SUM(A1,A2)");
        println!("{:?}", result);
    }

    #[test]
    fn names_vs_cell_refs() {
        use Token::*;
        // A cell reference (letters then digits) tokenizes as CellRef.
        assert_eq!(tokenize("A1"), vec![CellRef("A1".into())]);
        assert_eq!(tokenize("$AA$100"), vec![CellRef("$AA$100".into())]);
        // A bare identifier (no trailing digits) is a named-range reference.
        assert_eq!(tokenize("Revenue"), vec![Name("REVENUE".into())]);
        // Inside a function call, names still resolve as names.
        assert_eq!(
            tokenize("SUM(Rev)"),
            vec![Function("SUM".into()), LeftParen, Name("REV".into()), RightParen]
        );
        // An identifier followed by '(' is a function, not a name.
        assert!(matches!(tokenize("MAX(1)")[0], Function(_)));
    }

    // --- Cross-sheet references (issue #4) ---

    #[test]
    fn sheet_qualified_cell_ref() {
        use Token::*;
        assert_eq!(
            tokenize("Sheet2!A1"),
            vec![SheetCellRef { sheet: "SHEET2".into(), ref_: "A1".into() }]
        );
        assert_eq!(
            tokenize("Sheet2!$A$1"),
            vec![SheetCellRef { sheet: "SHEET2".into(), ref_: "$A$1".into() }]
        );
        // Mixed-locks across the sheet boundary.
        assert_eq!(
            tokenize("Sheet2!A$1"),
            vec![SheetCellRef { sheet: "SHEET2".into(), ref_: "A$1".into() }]
        );
    }

    #[test]
    fn sheet_qualified_range() {
        use Token::*;
        assert_eq!(
            tokenize("Sheet2!A1:B3"),
            vec![SheetRange { sheet: "SHEET2".into(), from: "A1".into(), to: "B3".into() }]
        );
    }

    #[test]
    fn sheet_qualified_in_expression() {
        use Token::*;
        assert_eq!(
            tokenize("Sheet2!A1 + 1"),
            vec![
                SheetCellRef { sheet: "SHEET2".into(), ref_: "A1".into() },
                Operator("+".into()),
                Number(1.0),
            ]
        );
        assert_eq!(
            tokenize("SUM(Sheet2!A1:A3)"),
            vec![
                Function("SUM".into()),
                LeftParen,
                SheetRange { sheet: "SHEET2".into(), from: "A1".into(), to: "A3".into() },
                RightParen,
            ]
        );
    }

    #[test]
    fn unprefixed_name_still_works() {
        use Token::*;
        // A bare identifier (no trailing digits) is still a named-range
        // reference; the sheet-prefix path is opt-in via `!`.
        assert_eq!(tokenize("SheetQ"), vec![Name("SHEETQ".into())]);
        // `Sheet2` matches the cell-ref pattern (letters + digits), so it
        // tokenizes as a CellRef on its own — it only becomes a sheet name
        // when followed by `!`. (This matches the pre-issue behavior; the
        // test guards against accidentally widening the cell-ref rule.)
        assert_eq!(tokenize("Sheet2"), vec![CellRef("SHEET2".into())]);
    }
}