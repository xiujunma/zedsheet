use crate::formula::functions::FormulaFunctions;
use crate::formula::parser::Token;
use std::collections::HashMap;

pub struct FormulaEvaluator {
    cell_values: HashMap<String, f64>,
}

impl Default for FormulaEvaluator {
    fn default() -> Self {
        FormulaEvaluator {
            cell_values: HashMap::new(),
        }
    }
}

impl FormulaEvaluator {
    pub fn new() -> Self {
        FormulaEvaluator::default()
    }

    pub fn set_cell_value(&mut self, cell_ref: &str, value: f64) {
        self.cell_values.insert(cell_ref.to_string(), value);
    }

    pub fn get_cell_value(&self, cell_ref: &str) -> f64 {
        self.cell_values.get(cell_ref).cloned().unwrap_or(0.0)
    }

    pub fn evaluate(&self, formula: &str) -> Result<f64, String> {
        if formula.starts_with('=') {
            let expr = &formula[1..];
            let tokens = crate::formula::parser::tokenize(expr);
            self.eval_tokens(&tokens)
        } else {
            // Plain number or expression
            if let Ok(num) = formula.parse::<f64>() {
                Ok(num)
            } else {
                Ok(0.0)
            }
        }
    }

    fn eval_tokens(&self, tokens: &[Token]) -> Result<f64, String> {
        let mut stack: Vec<f64> = Vec::new();
        let mut func_args: Vec<Vec<f64>> = Vec::new();
        let mut current_func: Option<String> = None;
        let mut i = 0;

        while i < tokens.len() {
            match &tokens[i] {
                Token::Number(n) => {
                    stack.push(*n);
                }
                Token::String(s) => {
                    // Handle string in formula context
                    if let Ok(num) = s.parse::<f64>() {
                        stack.push(num);
                    }
                }
                Token::CellRef(ref cell) => {
                    let value = self.get_cell_value(cell);
                    stack.push(value);
                }
                Token::Function(ref name) => {
                    current_func = Some(name.clone());
                    // Look ahead to find arguments
                    let mut arg_count = 0;
                    let mut j = i + 2; // Skip function name and '('
                    let mut paren_depth = 1;
                    while j < tokens.len() && paren_depth > 0 {
                        match &tokens[j] {
                            Token::LeftParen => { paren_depth += 1; }
                            Token::RightParen => { paren_depth -= 1; if paren_depth == 0 { break; } }
                            Token::Comma if paren_depth == 1 => { arg_count += 1; }
                            _ => {}
                        }
                        j += 1;
                    }
                    arg_count += 1; // Last argument

                    // Collect arguments from stack
                    let mut args: Vec<f64> = Vec::new();
                    for _ in 0..arg_count {
                        if let Some(val) = stack.pop() {
                            args.push(val);
                        }
                    }
                    args.reverse();

                    let result = self.call_function(name, &args);
                    stack.push(result);
                    i = j + 1;
                    continue;
                }
                Token::Operator(ref op) => {
                    match op.as_str() {
                        "+" => {
                            let b = stack.pop().unwrap_or(0.0);
                            let a = stack.pop().unwrap_or(0.0);
                            stack.push(a + b);
                        }
                        "-" => {
                            let b = stack.pop().unwrap_or(0.0);
                            let a = stack.pop().unwrap_or(0.0);
                            // If we only have one value, it's a unary minus
                            if stack.is_empty() && i == 0 {
                                stack.push(-b);
                            } else {
                                stack.push(a - b);
                            }
                        }
                        "*" => {
                            let b = stack.pop().unwrap_or(0.0);
                            let a = stack.pop().unwrap_or(0.0);
                            stack.push(a * b);
                        }
                        "/" => {
                            let b = stack.pop().unwrap_or(0.0);
                            let a = stack.pop().unwrap_or(1.0);
                            if b == 0.0 {
                                return Err("Division by zero".to_string());
                            }
                            stack.push(a / b);
                        }
                        "=" | "==" => {
                            let b = stack.pop().unwrap_or(0.0);
                            let a = stack.pop().unwrap_or(0.0);
                            stack.push(if a == b { 1.0 } else { 0.0 });
                        }
                        ">" => {
                            let b = stack.pop().unwrap_or(0.0);
                            let a = stack.pop().unwrap_or(0.0);
                            stack.push(if a > b { 1.0 } else { 0.0 });
                        }
                        ">=" => {
                            let b = stack.pop().unwrap_or(0.0);
                            let a = stack.pop().unwrap_or(0.0);
                            stack.push(if a >= b { 1.0 } else { 0.0 });
                        }
                        "<" => {
                            let b = stack.pop().unwrap_or(0.0);
                            let a = stack.pop().unwrap_or(0.0);
                            stack.push(if a < b { 1.0 } else { 0.0 });
                        }
                        "<=" => {
                            let b = stack.pop().unwrap_or(0.0);
                            let a = stack.pop().unwrap_or(0.0);
                            stack.push(if a <= b { 1.0 } else { 0.0 });
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
            i += 1;
        }

        stack.pop().ok_or_else(|| "Empty stack".to_string())
    }

    fn call_function(&self, name: &str, args: &[f64]) -> f64 {
        match name {
            "SUM" => FormulaFunctions::sum(args),
            "AVERAGE" => FormulaFunctions::average(args),
            "MAX" => FormulaFunctions::max(args),
            "MIN" => FormulaFunctions::min(args),
            "IF" => {
                if args.len() >= 3 {
                    FormulaFunctions::if_(args[0] != 0.0, args[1], args[2])
                } else {
                    0.0
                }
            }
            "AND" => {
                let bools: Vec<bool> = args.iter().map(|&v| v != 0.0).collect();
                FormulaFunctions::and(&bools) as i32 as f64
            }
            "OR" => {
                let bools: Vec<bool> = args.iter().map(|&v| v != 0.0).collect();
                FormulaFunctions::or(&bools) as i32 as f64
            }
            "CONCAT" => {
                // For CONCAT with numbers, just return sum
                FormulaFunctions::sum(args)
            }
            _ => 0.0,
        }
    }
}

/// Evaluate a suffix expression
pub fn eval_suffix_expr(
    src_stack: &[String],
    cell_getter: impl Fn(&str) -> f64,
) -> f64 {
    let mut stack: Vec<f64> = Vec::new();
    let mut cell_list: Vec<String> = Vec::new();

    for expr in src_stack {
        if expr.starts_with('"') {
            // String literal
            let s = expr.trim_start_matches('"');
            if let Ok(num) = s.parse::<f64>() {
                stack.push(num);
            }
        } else if expr.starts_with('[') {
            // Function call: [FUNC_NAME, arg_count]
            // Parse and evaluate
            continue;
        } else if expr == "+" {
            let b = stack.pop().unwrap_or(0.0);
            let a = stack.pop().unwrap_or(0.0);
            stack.push(a + b);
        } else if expr == "-" {
            let b = stack.pop().unwrap_or(0.0);
            let a = stack.pop().unwrap_or(0.0);
            stack.push(a - b);
        } else if expr == "*" {
            let b = stack.pop().unwrap_or(0.0);
            let a = stack.pop().unwrap_or(0.0);
            stack.push(a * b);
        } else if expr == "/" {
            let b = stack.pop().unwrap_or(1.0);
            let a = stack.pop().unwrap_or(0.0);
            if b != 0.0 {
                stack.push(a / b);
            }
        } else if expr == "=" || expr == ">" || expr == "<" || expr == ">=" || expr == "<=" {
            let b = stack.pop().unwrap_or(0.0);
            let a = stack.pop().unwrap_or(0.0);
            let result = match expr.as_str() {
                "=" => a == b,
                ">" => a > b,
                "<" => a < b,
                ">=" => a >= b,
                "<=" => a <= b,
                _ => false,
            };
            stack.push(if result { 1.0 } else { 0.0 });
        } else {
            // Cell reference or number
            if let Ok(num) = expr.parse::<f64>() {
                stack.push(num);
            } else {
                // Cell reference
                if cell_list.contains(expr) {
                    return 0.0; // Circular reference
                }
                cell_list.push(expr.clone());
                let value = cell_getter(expr);
                stack.push(value);
                cell_list.pop();
            }
        }
    }

    stack.first().cloned().unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_eval() {
        let mut eval = FormulaEvaluator::new();
        eval.set_cell_value("A1", 10.0);
        eval.set_cell_value("B2", 20.0);

        let result = eval.evaluate("=A1+B2");
        println!("Result: {:?}", result);
    }
}