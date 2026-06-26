use sha2::{Digest, Sha256};

use crate::types::{CellValue, FileData, MergeRange, SheetData};

pub type ContentHash = [u8; 32];

pub fn hash_file_content(file_data: &FileData) -> ContentHash {
    let mut hasher = Sha256::new();
    write_usize(&mut hasher, file_data.sheets.len());
    for sheet in &file_data.sheets {
        hash_sheet_content(sheet, &mut hasher);
    }
    hasher.finalize().into()
}

fn hash_sheet_content(sheet: &SheetData, hasher: &mut Sha256) {
    write_str(hasher, &sheet.name);
    write_usize(hasher, sheet.rows.len());
    for row in &sheet.rows {
        write_usize(hasher, row.len());
        for cell in row {
            hash_cell_value(cell, hasher);
        }
    }
    write_usize(hasher, sheet.merges.len());
    for merge in &sheet.merges {
        hash_merge_range(merge, hasher);
    }
    hash_layout_map(sheet.column_widths.as_ref(), hasher);
    hash_layout_map(sheet.row_heights.as_ref(), hasher);
}

fn hash_cell_value(cell: &CellValue, hasher: &mut Sha256) {
    match cell {
        CellValue::Null => {
            write_tag(hasher, 0);
        }
        CellValue::String(value) => {
            write_tag(hasher, 1);
            write_str(hasher, value);
        }
        CellValue::Number(value) => {
            write_tag(hasher, 2);
            write_str(hasher, &value.to_string());
        }
        CellValue::Boolean(value) => {
            write_tag(hasher, 3);
            hasher.update([u8::from(*value)]);
        }
        CellValue::Formula { formula, .. } => {
            write_tag(hasher, 4);
            write_str(hasher, formula);
        }
    }
}

fn hash_merge_range(merge: &MergeRange, hasher: &mut Sha256) {
    write_u32(hasher, merge.start_row);
    write_u16(hasher, merge.start_col);
    write_u32(hasher, merge.end_row);
    write_u16(hasher, merge.end_col);
}

fn hash_layout_map(map: Option<&std::collections::HashMap<usize, u32>>, hasher: &mut Sha256) {
    let Some(map) = map else {
        write_usize(hasher, 0);
        return;
    };
    let mut entries: Vec<_> = map.iter().collect();
    entries.sort_by_key(|(index, _)| *index);
    write_usize(hasher, entries.len());
    for (index, value) in entries {
        write_usize(hasher, *index);
        write_u32(hasher, *value);
    }
}

fn write_tag(hasher: &mut Sha256, tag: u8) {
    hasher.update([tag]);
}

fn write_str(hasher: &mut Sha256, value: &str) {
    write_usize(hasher, value.len());
    hasher.update(value.as_bytes());
}

fn write_usize(hasher: &mut Sha256, value: usize) {
    hasher.update(value.to_le_bytes());
}

fn write_u32(hasher: &mut Sha256, value: u32) {
    hasher.update(value.to_le_bytes());
}

fn write_u16(hasher: &mut Sha256, value: u16) {
    hasher.update(value.to_le_bytes());
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
    fn hash_ignores_file_identity() {
        let original = file_data();
        let mut changed_layout = original.clone();
        changed_layout.path = "/tmp/renamed.xlsx".to_string();
        changed_layout.file_name = "renamed.xlsx".to_string();

        assert_eq!(
            hash_file_content(&original),
            hash_file_content(&changed_layout)
        );
    }

    #[test]
    fn hash_changes_when_persisted_layout_changes() {
        let original = file_data();
        let mut changed_layout = original.clone();
        changed_layout.sheets[0].column_widths = Some(HashMap::from([(0, 240)]));
        changed_layout.sheets[0].row_heights = Some(HashMap::from([(0, 96)]));

        assert_ne!(
            hash_file_content(&original),
            hash_file_content(&changed_layout)
        );
    }

    #[test]
    fn hash_changes_when_saved_content_changes() {
        let original = file_data();
        let mut changed_content = original.clone();
        changed_content.sheets[0].rows[0][0] = CellValue::String("b".to_string());

        assert_ne!(
            hash_file_content(&original),
            hash_file_content(&changed_content)
        );
    }

    #[test]
    fn formula_hash_uses_formula_text_not_cached_value() {
        let mut original = file_data();
        original.sheets[0].rows[0][0] = CellValue::formula("=A2+1", CellValue::Number(1.into()));

        let mut changed_cache = original.clone();
        changed_cache.sheets[0].rows[0][0] =
            CellValue::formula("=A2+1", CellValue::Number(2.into()));

        let mut changed_formula = original.clone();
        changed_formula.sheets[0].rows[0][0] =
            CellValue::formula("=A2+2", CellValue::Number(1.into()));

        assert_eq!(
            hash_file_content(&original),
            hash_file_content(&changed_cache)
        );
        assert_ne!(
            hash_file_content(&original),
            hash_file_content(&changed_formula)
        );
    }
}
