use crate::core::helper::number_calc;

pub struct FormulaFunctions;

impl FormulaFunctions {
    pub fn sum(values: &[f64]) -> f64 {
        values.iter().fold(0.0, |acc, &v| {
            let result = number_calc("+", acc, v);
            result.parse().unwrap_or(acc)
        })
    }

    pub fn average(values: &[f64]) -> f64 {
        if values.is_empty() {
            return 0.0;
        }
        let sum: f64 = values.iter().fold(0.0, |acc, &v| {
            let result = number_calc("+", acc, v);
            result.parse().unwrap_or(acc)
        });
        sum / values.len() as f64
    }

    pub fn max(values: &[f64]) -> f64 {
        values.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
    }

    pub fn min(values: &[f64]) -> f64 {
        values.iter().cloned().fold(f64::INFINITY, f64::min)
    }

    pub fn if_(condition: bool, then_val: f64, else_val: f64) -> f64 {
        if condition { then_val } else { else_val }
    }

    pub fn and(values: &[bool]) -> bool {
        values.iter().all(|&v| v)
    }

    pub fn or(values: &[bool]) -> bool {
        values.iter().any(|&v| v)
    }

    pub fn concat(values: &[String]) -> String {
        values.join("")
    }
}

// Convenience functions
pub fn sum(values: &[f64]) -> f64 {
    FormulaFunctions::sum(values)
}

pub fn average(values: &[f64]) -> f64 {
    FormulaFunctions::average(values)
}

pub fn max(values: &[f64]) -> f64 {
    FormulaFunctions::max(values)
}

pub fn min(values: &[f64]) -> f64 {
    FormulaFunctions::min(values)
}

pub fn if_(condition: bool, then_val: f64, else_val: f64) -> f64 {
    FormulaFunctions::if_(condition, then_val, else_val)
}

pub fn and(values: &[bool]) -> bool {
    FormulaFunctions::and(values)
}

pub fn or(values: &[bool]) -> bool {
    FormulaFunctions::or(values)
}

pub fn concat(values: &[String]) -> String {
    FormulaFunctions::concat(values)
}