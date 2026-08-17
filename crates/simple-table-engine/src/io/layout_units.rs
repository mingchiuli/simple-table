use crate::document_layout_policy::{DEFAULT_COLUMN_WIDTH_PX, DEFAULT_ROW_HEIGHT_PX};

const EXCEL_DEFAULT_COLUMN_WIDTH: f64 = 8.38;

pub fn excel_column_width_to_px(width: f64) -> u32 {
    if width <= 0.0 {
        return DEFAULT_COLUMN_WIDTH_PX;
    }
    ((width * 7.0) + 5.0).round().max(1.0) as u32
}

pub fn px_to_excel_column_width(px: u32) -> f64 {
    px.saturating_sub(5) as f64 / 7.0
}

pub fn is_default_column_width(width: f64, _px: u32) -> bool {
    // Only the native Excel default column width is dropped; an explicit
    // width at the engine default (120px) must survive a round-trip so that
    // an inserted image sized exactly 120px keeps its column width.
    (width - EXCEL_DEFAULT_COLUMN_WIDTH).abs() < 0.001
}

pub fn points_to_px(points: f64) -> u32 {
    if points <= 0.0 {
        return DEFAULT_ROW_HEIGHT_PX;
    }
    (points * 96.0 / 72.0).round().max(1.0) as u32
}

pub fn px_to_points(px: u32) -> f64 {
    px as f64 * 72.0 / 96.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn column_width_conversion_is_stable_for_ui_default() {
        assert_eq!(excel_column_width_to_px(16.428571428571427), 120);
    }

    #[test]
    fn default_umya_column_width_is_not_persisted_as_custom_layout() {
        assert!(is_default_column_width(
            8.38,
            excel_column_width_to_px(8.38)
        ));
    }

    #[test]
    fn row_height_conversion_uses_pixels() {
        assert_eq!(points_to_px(54.0), 72);
    }
}
