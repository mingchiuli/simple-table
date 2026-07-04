use crate::io::document_model::FileStructureMemento;
use crate::types::{
    CellValue, ColumnsDeletedPatch, ColumnsInsertedPatch, EditorPatch, FileData, RowsDeletedPatch,
    RowsInsertedPatch, SheetData, SheetDeletedPatch, SheetInsertedPatch, SheetMetadataPatch,
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
                return row_structure_patch_from(
                    restored,
                    target.sheet_index,
                    target.row_index,
                    target_count > *row_count,
                );
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
                return column_structure_patch_from(
                    restored,
                    target.sheet_index,
                    target.col_index,
                    target.row_lengths.iter().sum::<usize>() > row_lengths.iter().sum::<usize>(),
                );
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

fn row_structure_patch_from(
    restored: &FileData,
    sheet_index: usize,
    row_index: usize,
    inserted: bool,
) -> Vec<EditorPatch> {
    restored
        .sheets
        .get(sheet_index)
        .map(|sheet| {
            let mut patches = if inserted {
                vec![EditorPatch::RowsInserted {
                    patch: RowsInsertedPatch {
                        sheet_index,
                        row_index,
                        rows: sheet
                            .rows
                            .get(row_index)
                            .cloned()
                            .map(|row| vec![row])
                            .unwrap_or_default(),
                        display_formats: Vec::new(),
                    },
                }]
            } else {
                vec![EditorPatch::RowsDeleted {
                    patch: RowsDeletedPatch {
                        sheet_index,
                        row_index,
                        count: 1,
                    },
                }]
            };
            patches.push(sheet_metadata_patch(sheet_index, sheet));
            patches
        })
        .unwrap_or_default()
}

fn column_structure_patch_from(
    restored: &FileData,
    sheet_index: usize,
    col_index: usize,
    inserted: bool,
) -> Vec<EditorPatch> {
    restored
        .sheets
        .get(sheet_index)
        .map(|sheet| {
            let mut patches = if inserted {
                vec![EditorPatch::ColumnsInserted {
                    patch: ColumnsInsertedPatch {
                        sheet_index,
                        col_index,
                        values: sheet
                            .rows
                            .iter()
                            .map(|row| row.get(col_index).cloned().unwrap_or(CellValue::Null))
                            .collect(),
                        display_formats: Vec::new(),
                    },
                }]
            } else {
                vec![EditorPatch::ColumnsDeleted {
                    patch: ColumnsDeletedPatch {
                        sheet_index,
                        col_index,
                        count: 1,
                    },
                }]
            };
            patches.push(sheet_metadata_patch(sheet_index, sheet));
            patches
        })
        .unwrap_or_default()
}

fn sheet_metadata_patch(sheet_index: usize, sheet: &SheetData) -> EditorPatch {
    EditorPatch::SheetMetadata {
        patch: SheetMetadataPatch {
            sheet_index,
            merges: sheet.merges.clone(),
            column_widths: sheet.column_widths.clone().unwrap_or_default(),
            row_heights: sheet.row_heights.clone().unwrap_or_default(),
            rich: sheet.rich.clone(),
        },
    }
}
