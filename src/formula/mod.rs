pub mod parser;
pub mod evaluator;
pub mod functions;

pub use parser::{infix_expr_to_suffix_expr, FormulaParser, Token};
pub use evaluator::{FormulaEvaluator, eval_suffix_expr};
pub use functions::{FormulaFunctions, sum, average, max, min, if_ as if_func, and, or, concat};