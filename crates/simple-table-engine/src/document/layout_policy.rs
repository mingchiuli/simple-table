//! Layout dimension bounds and conversion fallbacks.
//!
//! The `FALLBACK_*` constants are used when an Excel source reports no usable
//! size (`<= 0`). They are not the UI defaults: the app owns the rendered
//! defaults (see `apps/simple-table/src/components/grid.rs`).

/// Width used when an Excel column reports no usable width.
pub const FALLBACK_COLUMN_WIDTH_PX: u32 = 120;
/// Height used when an Excel row reports no usable height.
pub const FALLBACK_ROW_HEIGHT_PX: u32 = 72;

pub const MIN_COLUMN_WIDTH_PX: u32 = 1;
pub const MAX_COLUMN_WIDTH_PX: u32 = 4_096;
pub const MIN_ROW_HEIGHT_PX: u32 = 1;
pub const MAX_ROW_HEIGHT_PX: u32 = 4_096;

pub fn is_supported_column_width(width: u32) -> bool {
    (MIN_COLUMN_WIDTH_PX..=MAX_COLUMN_WIDTH_PX).contains(&width)
}

pub fn is_supported_row_height(height: u32) -> bool {
    (MIN_ROW_HEIGHT_PX..=MAX_ROW_HEIGHT_PX).contains(&height)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_layout_dimensions_have_explicit_bounds() {
        assert!(is_supported_column_width(MIN_COLUMN_WIDTH_PX));
        assert!(is_supported_column_width(MAX_COLUMN_WIDTH_PX));
        assert!(!is_supported_column_width(0));
        assert!(!is_supported_column_width(MAX_COLUMN_WIDTH_PX + 1));

        assert!(is_supported_row_height(MIN_ROW_HEIGHT_PX));
        assert!(is_supported_row_height(MAX_ROW_HEIGHT_PX));
        assert!(!is_supported_row_height(0));
        assert!(!is_supported_row_height(MAX_ROW_HEIGHT_PX + 1));
    }
}
