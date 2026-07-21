pub const DEFAULT_COLUMN_WIDTH_PX: u32 = 120;
pub const DEFAULT_ROW_HEIGHT_PX: u32 = 72;

pub const MIN_COLUMN_WIDTH_PX: u32 = 1;
pub const MAX_COLUMN_WIDTH_PX: u32 = 4_096;
pub const MIN_ROW_HEIGHT_PX: u32 = 1;
pub const MAX_ROW_HEIGHT_PX: u32 = 4_096;

#[cfg_attr(not(test), allow(dead_code))]
pub const MIN_INTERACTIVE_COLUMN_WIDTH_PX: u32 = 56;
#[cfg_attr(not(test), allow(dead_code))]
pub const MIN_INTERACTIVE_ROW_HEIGHT_PX: u32 = 36;

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
