use serde_json::Value;

use crate::types::{CellFormatProjection, CellValue};

pub(crate) struct DisplayProjection;

impl DisplayProjection {
    pub(crate) fn display_text(cell: &CellValue, format: Option<&CellFormatProjection>) -> String {
        match cell {
            CellValue::Formula {
                cached_value,
                error,
                ..
            } => error
                .clone()
                .unwrap_or_else(|| Self::display_text(cached_value, format)),
            CellValue::Number(number) => format
                .and_then(|format| format.number_format.as_deref())
                .and_then(|pattern| format_number_with_excel_pattern(number, pattern))
                .unwrap_or_else(|| raw_text(cell)),
            _ => raw_text(cell),
        }
    }

    pub(crate) fn search_text(cell: &CellValue, format: Option<&CellFormatProjection>) -> String {
        let display = Self::display_text(cell, format);
        let raw = raw_text(cell);
        if raw.is_empty() || raw == display {
            display
        } else if display.is_empty() {
            raw
        } else {
            format!("{display}\n{raw}")
        }
    }
}

fn raw_text(cell: &CellValue) -> String {
    match cell {
        CellValue::Null => String::new(),
        CellValue::String(value) => value.clone(),
        CellValue::Number(value) => value.to_string(),
        CellValue::Boolean(value) => value.to_string(),
        CellValue::Formula {
            cached_value,
            error,
            ..
        } => error.clone().unwrap_or_else(|| raw_text(cached_value)),
    }
}

fn format_number_with_excel_pattern(number: &Value, pattern: &str) -> Option<String> {
    let value = number.as_f64()?;
    if !value.is_finite() {
        return None;
    }

    let normalized = pattern.to_ascii_lowercase();
    if normalized.contains("yy") || normalized.contains("dd") || normalized.contains("m/") {
        return format_excel_date(value, pattern);
    }

    let percent = pattern.contains('%');
    let displayed = if percent { value * 100.0 } else { value };
    let decimals = decimal_places(pattern);
    let formatted = format_fixed_number(displayed, decimals, pattern.contains(','));
    let currency = pattern
        .chars()
        .find(|value| matches!(value, '$' | '¥' | '￥' | '€' | '£'))
        .map(|value| value.to_string())
        .unwrap_or_default();

    Some(format!(
        "{currency}{formatted}{}",
        if percent { "%" } else { "" }
    ))
}

fn decimal_places(pattern: &str) -> usize {
    let Some(decimal_start) = pattern.find('.') else {
        return 0;
    };
    pattern[decimal_start + 1..]
        .chars()
        .take_while(|value| matches!(value, '0' | '#'))
        .count()
}

fn format_fixed_number(value: f64, decimals: usize, use_grouping: bool) -> String {
    let formatted = format!("{value:.decimals$}");
    if !use_grouping {
        return formatted;
    }

    let (negative, unsigned) = formatted
        .strip_prefix('-')
        .map(|value| (true, value))
        .unwrap_or((false, formatted.as_str()));
    let (integer, fraction) = unsigned
        .split_once('.')
        .map(|(integer, fraction)| (integer, Some(fraction)))
        .unwrap_or((unsigned, None));

    let mut grouped = String::new();
    for (index, ch) in integer.chars().rev().enumerate() {
        if index > 0 && index % 3 == 0 {
            grouped.insert(0, ',');
        }
        grouped.insert(0, ch);
    }

    if negative {
        grouped.insert(0, '-');
    }
    if let Some(fraction) = fraction {
        grouped.push('.');
        grouped.push_str(fraction);
    }
    grouped
}

fn format_excel_date(value: f64, pattern: &str) -> Option<String> {
    let days = value.round() as i64;
    let (year, month, day) = excel_serial_date_to_ymd(days)?;
    if pattern.contains('/') {
        Some(format!("{year:04}/{month:02}/{day:02}"))
    } else {
        Some(format!("{year:04}-{month:02}-{day:02}"))
    }
}

fn excel_serial_date_to_ymd(days: i64) -> Option<(i32, u32, u32)> {
    civil_from_days(days - 25_569)
}

fn civil_from_days(days: i64) -> Option<(i32, u32, u32)> {
    let days = days + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    let year = year + i64::from(month <= 2);
    Some((i32::try_from(year).ok()?, month as u32, day as u32))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_percent_display_text() {
        let cell = CellValue::Number(Value::from(0.4));
        let format = CellFormatProjection {
            number_format: Some("0%".to_string()),
            style_id: None,
        };

        assert_eq!(DisplayProjection::display_text(&cell, Some(&format)), "40%");
    }

    #[test]
    fn search_text_includes_display_and_raw_number() {
        let cell = CellValue::Number(Value::from(0.4));
        let format = CellFormatProjection {
            number_format: Some("0%".to_string()),
            style_id: None,
        };

        assert_eq!(
            DisplayProjection::search_text(&cell, Some(&format)),
            "40%\n0.4"
        );
    }
}
