use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use crate::types::{CellValue, FileData, MergeRange, SheetData};

pub fn hash_file_content(file_data: &FileData) -> u64 {
    let mut hasher = DefaultHasher::new();
    file_data.sheets.len().hash(&mut hasher);
    for sheet in &file_data.sheets {
        hash_sheet_content(sheet, &mut hasher);
    }
    hasher.finish()
}

fn hash_sheet_content(sheet: &SheetData, hasher: &mut DefaultHasher) {
    sheet.name.hash(hasher);
    sheet.rows.len().hash(hasher);
    for row in &sheet.rows {
        row.len().hash(hasher);
        for cell in row {
            hash_cell_value(cell, hasher);
        }
    }
    sheet.merges.len().hash(hasher);
    for merge in &sheet.merges {
        hash_merge_range(merge, hasher);
    }
}

fn hash_cell_value(cell: &CellValue, hasher: &mut DefaultHasher) {
    match cell {
        CellValue::Null => {
            0_u8.hash(hasher);
        }
        CellValue::String(value) => {
            1_u8.hash(hasher);
            value.hash(hasher);
        }
        CellValue::Number(value) => {
            2_u8.hash(hasher);
            value.to_string().hash(hasher);
        }
        CellValue::Boolean(value) => {
            3_u8.hash(hasher);
            value.hash(hasher);
        }
    }
}

fn hash_merge_range(merge: &MergeRange, hasher: &mut DefaultHasher) {
    merge.start_row.hash(hasher);
    merge.start_col.hash(hasher);
    merge.end_row.hash(hasher);
    merge.end_col.hash(hasher);
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::hash_file_content;
    use crate::types::{CellValue, FileData, SheetData};

    fn file_data() -> FileData {
        FileData {
            path: "/tmp/source.xlsx".to_string(),
            file_name: "source.xlsx".to_string(),
            sheets: vec![SheetData {
                name: "Sheet1".to_string(),
                rows: vec![vec![CellValue::String("a".to_string())]],
                merges: vec![],
                ..Default::default()
            }],
        }
    }

    #[test]
    fn hash_ignores_runtime_and_layout_fields() {
        let original = file_data();
        let mut changed_layout = original.clone();
        changed_layout.path = "/tmp/renamed.xlsx".to_string();
        changed_layout.file_name = "renamed.xlsx".to_string();
        changed_layout.sheets[0].column_widths = Some(HashMap::from([(0, 240)]));

        assert_eq!(hash_file_content(&original), hash_file_content(&changed_layout));
    }

    #[test]
    fn hash_changes_when_saved_content_changes() {
        let original = file_data();
        let mut changed_content = original.clone();
        changed_content.sheets[0].rows[0][0] = CellValue::String("b".to_string());

        assert_ne!(hash_file_content(&original), hash_file_content(&changed_content));
    }
}
