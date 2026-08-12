use formualizer_workbook::LiteralValue;

use crate::domain::{CellNumber, CellValue};

pub(crate) fn to_formula_index(index: usize) -> u32 {
    index.saturating_add(1) as u32
}

pub(crate) fn cell_to_literal(cell: &CellValue) -> LiteralValue {
    match cell {
        CellValue::Null => LiteralValue::Empty,
        CellValue::String(value) => LiteralValue::Text(value.clone()),
        CellValue::Number(value) => {
            if let Some(int) = value.as_i64() {
                LiteralValue::Int(int)
            } else {
                LiteralValue::Number(value.as_f64())
            }
        }
        CellValue::Boolean(value) => LiteralValue::Boolean(*value),
        CellValue::Formula { cached_value, .. } => cell_to_literal(cached_value),
    }
}

pub(crate) fn literal_to_cell(value: LiteralValue) -> (CellValue, Option<String>) {
    match value {
        LiteralValue::Empty | LiteralValue::Pending => (CellValue::Null, None),
        LiteralValue::Int(value) => (CellValue::Number(CellNumber::from(value)), None),
        LiteralValue::Number(value) => (
            CellNumber::from_f64(value)
                .map(CellValue::Number)
                .unwrap_or_else(|| CellValue::String(value.to_string())),
            None,
        ),
        LiteralValue::Text(value) => (CellValue::String(value), None),
        LiteralValue::Boolean(value) => (CellValue::Boolean(value), None),
        LiteralValue::Error(error) => (CellValue::Null, Some(error.kind.to_string())),
        LiteralValue::Array(values) => values
            .first()
            .and_then(|row| row.first())
            .cloned()
            .map(literal_to_cell)
            .unwrap_or((CellValue::Null, None)),
        LiteralValue::Date(value) => (CellValue::String(value.to_string()), None),
        LiteralValue::DateTime(value) => (CellValue::String(value.to_string()), None),
        LiteralValue::Time(value) => (CellValue::String(value.to_string()), None),
        LiteralValue::Duration(value) => (CellValue::String(value.to_string()), None),
    }
}
