use crate::io::document_model::FileStructureMemento;
use crate::types::{
    EditorPatch, FileData, SheetDeletedPatch, SheetInsertedPatch, SheetUpdatedPatch,
};

pub(crate) enum CurrentStructureShape {
    Empty,
    Row {
        sheet_index: usize,
        row_index: usize,
        row_count: usize,
    },
    Column {
        sheet_index: usize,
        column_index: usize,
        row_lengths: Vec<usize>,
    },
    Sheets {
        sheet_index: usize,
        sheet_count: usize,
    },
}

impl CurrentStructureShape {
    pub(crate) fn capture(file_data: &FileData, target: &FileStructureMemento) -> Self {
        match target {
            FileStructureMemento::Empty { .. } => Self::Empty,
            FileStructureMemento::Row(memento) => Self::Row {
                sheet_index: memento.sheet_index,
                row_index: memento.row_index,
                row_count: file_data
                    .sheets
                    .get(memento.sheet_index)
                    .map(|sheet| sheet.rows.len())
                    .unwrap_or_default(),
            },
            FileStructureMemento::Column(memento) => Self::Column {
                sheet_index: memento.sheet_index,
                column_index: memento.col_index,
                row_lengths: file_data
                    .sheets
                    .get(memento.sheet_index)
                    .map(|sheet| sheet.rows.iter().map(Vec::len).collect())
                    .unwrap_or_default(),
            },
            FileStructureMemento::Sheets(memento) => Self::Sheets {
                sheet_index: memento.truncate_from,
                sheet_count: file_data.sheets.len(),
            },
        }
    }
}

pub(crate) fn restore_structure_patches(
    current_shape: &CurrentStructureShape,
    target_memento: &FileStructureMemento,
    restored: &FileData,
) -> Vec<EditorPatch> {
    match (current_shape, target_memento) {
        (
            CurrentStructureShape::Row {
                sheet_index,
                row_index,
                row_count,
            },
            FileStructureMemento::Row(target),
        ) if *sheet_index == target.sheet_index && *row_index == target.row_index => {
            let target_count = target.row_count;
            if target_count != *row_count {
                return sheet_updated_patch_from(restored, target.sheet_index);
            }
            Vec::new()
        }
        (
            CurrentStructureShape::Column {
                sheet_index,
                column_index,
                row_lengths,
            },
            FileStructureMemento::Column(target),
        ) if *sheet_index == target.sheet_index && *column_index == target.col_index => {
            let changed = target
                .row_lengths
                .iter()
                .zip(row_lengths.iter().chain(std::iter::repeat(&0)))
                .any(|(target_len, current_len)| target_len != current_len)
                || row_lengths
                    .iter()
                    .zip(target.row_lengths.iter().chain(std::iter::repeat(&0)))
                    .any(|(current_len, target_len)| current_len != target_len);

            if changed {
                return sheet_updated_patch_from(restored, target.sheet_index);
            }
            Vec::new()
        }
        (
            CurrentStructureShape::Sheets {
                sheet_index,
                sheet_count,
            },
            FileStructureMemento::Sheets(target),
        ) if *sheet_index == target.truncate_from => {
            if target.sheet_count > *sheet_count {
                return restored
                    .sheets
                    .get(target.truncate_from)
                    .cloned()
                    .map(|sheet| {
                        vec![EditorPatch::SheetInserted {
                            patch: SheetInsertedPatch {
                                sheet_index: target.truncate_from,
                                sheet,
                            },
                        }]
                    })
                    .unwrap_or_default();
            }
            if target.sheet_count < *sheet_count {
                return vec![EditorPatch::SheetDeleted {
                    patch: SheetDeletedPatch {
                        sheet_index: target.truncate_from,
                    },
                }];
            }
            Vec::new()
        }
        _ => Vec::new(),
    }
}

fn sheet_updated_patch_from(restored: &FileData, sheet_index: usize) -> Vec<EditorPatch> {
    restored
        .sheets
        .get(sheet_index)
        .cloned()
        .map(|sheet| {
            vec![EditorPatch::SheetUpdated {
                patch: SheetUpdatedPatch { sheet_index, sheet },
            }]
        })
        .unwrap_or_default()
}
