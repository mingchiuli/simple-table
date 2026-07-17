use crate::io::document_memento::FileStructureMemento;
use crate::types::{
    ColumnDeletedPatch, ColumnInsertedPatch, EditorPatch, FileData, ResyncRequiredPatch,
    RowDeletedPatch, RowInsertedPatch, SheetLayoutProjection, SheetManifest, SheetsReplacedPatch,
};

pub(crate) enum CurrentStructureShape {
    Empty,
    Row { row_count: usize },
    Column { row_lengths: Vec<usize> },
    Sheets { sheet_index: usize },
}

impl CurrentStructureShape {
    pub(crate) fn capture(file_data: &FileData, target: &FileStructureMemento) -> Self {
        match target {
            FileStructureMemento::Empty { .. } => Self::Empty,
            FileStructureMemento::Row(target) => file_data
                .sheets
                .get(target.sheet_index)
                .map(|sheet| Self::Row {
                    row_count: sheet.rows.len(),
                })
                .unwrap_or(Self::Empty),
            FileStructureMemento::Column(target) => file_data
                .sheets
                .get(target.sheet_index)
                .map(|sheet| Self::Column {
                    row_lengths: sheet.rows.iter().map(Vec::len).collect(),
                })
                .unwrap_or(Self::Empty),
            FileStructureMemento::Sheets(memento) => Self::Sheets {
                sheet_index: memento.truncate_from,
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
        (CurrentStructureShape::Row { row_count }, FileStructureMemento::Row(target))
            if target.row_count > *row_count =>
        {
            vec![EditorPatch::RowInserted {
                patch: RowInsertedPatch {
                    sheet_index: target.sheet_index,
                    row_index: target.row_index,
                    count: target.row_count - *row_count,
                },
            }]
        }
        (CurrentStructureShape::Row { row_count }, FileStructureMemento::Row(target))
            if target.row_count < *row_count =>
        {
            vec![EditorPatch::RowDeleted {
                patch: RowDeletedPatch {
                    sheet_index: target.sheet_index,
                    row_index: target.row_index,
                    count: *row_count - target.row_count,
                },
            }]
        }
        (CurrentStructureShape::Column { row_lengths }, FileStructureMemento::Column(target)) => {
            column_restore_patch(row_lengths, target)
        }
        (CurrentStructureShape::Sheets { sheet_index }, FileStructureMemento::Sheets(target))
            if *sheet_index == target.truncate_from =>
        {
            vec![EditorPatch::SheetsReplaced {
                patch: SheetsReplacedPatch {
                    start_index: target.truncate_from,
                    sheets: restored
                        .sheets
                        .get(target.truncate_from..)
                        .unwrap_or_default()
                        .iter()
                        .map(|sheet| SheetManifest {
                            name: sheet.name.clone(),
                            extent: sheet.extent(),
                            layout: SheetLayoutProjection {
                                column_widths: sheet.column_widths.clone().unwrap_or_default(),
                                row_heights: sheet.row_heights.clone().unwrap_or_default(),
                            },
                        })
                        .collect(),
                },
            }]
        }
        (_, FileStructureMemento::Row(_)) | (_, FileStructureMemento::Column(_)) => {
            resync_structure_patch()
        }
        _ => Vec::new(),
    }
}

fn column_restore_patch(
    current_lengths: &[usize],
    target: &crate::io::document_memento::ColumnStructureMemento,
) -> Vec<EditorPatch> {
    let mut target_is_wider = false;
    let mut current_is_wider = false;
    let row_count = current_lengths.len().max(target.row_lengths.len());
    for row_index in 0..row_count {
        let current = current_lengths.get(row_index).copied().unwrap_or_default();
        let restored = target
            .row_lengths
            .get(row_index)
            .copied()
            .unwrap_or_default();
        target_is_wider |= restored > current;
        current_is_wider |= current > restored;
    }
    match (target_is_wider, current_is_wider) {
        (true, false) => vec![EditorPatch::ColumnInserted {
            patch: ColumnInsertedPatch {
                sheet_index: target.sheet_index,
                col_index: target.col_index,
                count: 1,
            },
        }],
        (false, true) => vec![EditorPatch::ColumnDeleted {
            patch: ColumnDeletedPatch {
                sheet_index: target.sheet_index,
                col_index: target.col_index,
                count: 1,
            },
        }],
        _ => resync_structure_patch(),
    }
}

fn resync_structure_patch() -> Vec<EditorPatch> {
    vec![EditorPatch::ResyncRequired {
        patch: ResyncRequiredPatch {
            reason: "structure restore direction could not be represented incrementally"
                .to_string(),
        },
    }]
}
