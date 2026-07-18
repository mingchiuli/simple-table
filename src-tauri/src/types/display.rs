use crate::domain::format_cell_display;
#[cfg(test)]
use crate::domain::format_cell_search;
use crate::types::{CellFormatProjection, CellValue};

pub(crate) struct DisplayProjection;

impl DisplayProjection {
    pub(crate) fn display_text(cell: &CellValue, format: Option<&CellFormatProjection>) -> String {
        format_cell_display(
            cell,
            format.and_then(|format| format.number_format.as_deref()),
        )
    }

    #[cfg(test)]
    pub(crate) fn search_text(cell: &CellValue, format: Option<&CellFormatProjection>) -> String {
        format_cell_search(
            cell,
            format.and_then(|format| format.number_format.as_deref()),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::CellNumber;

    #[test]
    fn formats_percent_display_text() {
        let cell = CellValue::Number(CellNumber::from_f64(0.4).unwrap());
        let format = CellFormatProjection {
            number_format: Some("0%".to_string()),
            style_id: None,
        };

        assert_eq!(DisplayProjection::display_text(&cell, Some(&format)), "40%");
    }

    #[test]
    fn search_text_includes_display_and_raw_number() {
        let cell = CellValue::Number(CellNumber::from_f64(0.4).unwrap());
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
