use crate::domain::{SearchCellText, SearchScanCursor, SearchTextChunk};
use crate::types::SheetData;

pub(crate) fn collect_sheet_search_text_chunk(
    sheet: &SheetData,
    mut cursor: SearchScanCursor,
    maximum_text_bytes: usize,
    maximum_cells: usize,
) -> SearchTextChunk {
    let mut cells = Vec::new();
    let mut text_bytes = 0usize;
    let mut visited_cells = 0usize;

    while cursor.row < sheet.rows.len() {
        let row = &sheet.rows[cursor.row];
        while cursor.col < row.len() {
            let search_text = sheet.cell_search_text(cursor.row, cursor.col);
            let display_text = sheet.cell_display_text(cursor.row, cursor.col);
            let cell_bytes = search_text.len().saturating_add(display_text.len());
            if !cells.is_empty() && text_bytes.saturating_add(cell_bytes) > maximum_text_bytes {
                return SearchTextChunk {
                    cells,
                    next: Some(cursor),
                };
            }
            if !search_text.is_empty() {
                cells.push(SearchCellText {
                    row: cursor.row,
                    col: cursor.col,
                    search_text,
                    display_text,
                });
                text_bytes = text_bytes.saturating_add(cell_bytes);
            }
            cursor.col += 1;
            visited_cells += 1;
            if visited_cells >= maximum_cells {
                return SearchTextChunk {
                    cells,
                    next: Some(cursor),
                };
            }
        }
        cursor.row += 1;
        cursor.col = 0;
    }

    SearchTextChunk { cells, next: None }
}
