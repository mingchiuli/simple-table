use serde_json::Value;

const JS_MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_991;
const JS_MIN_SAFE_INTEGER: i64 = -9_007_199_254_740_991;

#[derive(Clone, Debug, PartialEq)]
pub enum CellValue {
    Null,
    String(String),
    Number(Value),
    Boolean(bool),
    Formula {
        formula: String,
        cached_value: Box<CellValue>,
        error: Option<String>,
    },
}

impl CellValue {
    pub fn to_display_string(&self) -> String {
        match self {
            CellValue::Null => String::new(),
            CellValue::String(value) => value.clone(),
            CellValue::Number(value) => value.to_string(),
            CellValue::Boolean(value) => value.to_string(),
            CellValue::Formula {
                cached_value,
                error,
                ..
            } => error
                .clone()
                .unwrap_or_else(|| cached_value.to_display_string()),
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            CellValue::Null => "blank",
            CellValue::String(_) => "text",
            CellValue::Number(_) => "number",
            CellValue::Boolean(_) => "boolean",
            CellValue::Formula { error, .. } => {
                if error.is_some() {
                    "error"
                } else {
                    "formula"
                }
            }
        }
    }

    pub fn formula(formula: impl Into<String>, cached_value: CellValue) -> Self {
        CellValue::Formula {
            formula: normalize_formula_text(formula.into()),
            cached_value: Box::new(cached_value),
            error: None,
        }
    }

    pub fn with_formula_result(&self, cached_value: CellValue, error: Option<String>) -> Self {
        match self {
            CellValue::Formula { formula, .. } => CellValue::Formula {
                formula: formula.clone(),
                cached_value: Box::new(cached_value),
                error,
            },
            _ => self.clone(),
        }
    }
}

pub fn normalize_formula_text(formula: String) -> String {
    if formula.starts_with('=') {
        formula
    } else {
        format!("={formula}")
    }
}

pub fn parse_cell_text(text: &str) -> CellValue {
    if text.is_empty() {
        return CellValue::Null;
    }
    if text.starts_with('=') {
        return CellValue::formula(text, CellValue::Null);
    }
    if has_leading_zero(text) {
        return CellValue::String(text.to_string());
    }
    if let Ok(value) = text.parse::<i64>() {
        if (JS_MIN_SAFE_INTEGER..=JS_MAX_SAFE_INTEGER).contains(&value) {
            return CellValue::Number(Value::from(value));
        }
        return CellValue::String(text.to_string());
    }
    if let Ok(value) = text.parse::<f64>()
        && value.is_finite()
    {
        return CellValue::Number(Value::from(value));
    }
    if text.eq_ignore_ascii_case("true") {
        return CellValue::Boolean(true);
    }
    if text.eq_ignore_ascii_case("false") {
        return CellValue::Boolean(false);
    }
    CellValue::String(text.to_string())
}

fn has_leading_zero(text: &str) -> bool {
    let bytes = text.as_bytes();
    let digits = if bytes.first() == Some(&b'-') || bytes.first() == Some(&b'+') {
        &bytes[1..]
    } else {
        bytes
    };
    digits.len() > 1 && digits[0] == b'0' && digits.iter().all(|byte| byte.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_user_cell_text() {
        assert_eq!(parse_cell_text(""), CellValue::Null);
        assert_eq!(parse_cell_text("007"), CellValue::String("007".to_string()));
        assert_eq!(parse_cell_text("42"), CellValue::Number(Value::from(42)));
        assert_eq!(parse_cell_text("3.5"), CellValue::Number(Value::from(3.5)));
        assert_eq!(parse_cell_text("true"), CellValue::Boolean(true));
        assert_eq!(
            parse_cell_text("=A1+1"),
            CellValue::formula("=A1+1", CellValue::Null)
        );
    }
}
