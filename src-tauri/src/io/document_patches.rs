use crate::io::document_model::FileStructureMemento;
use crate::ops::patch_projector::sheet_updated_patch;
use crate::types::{EditorPatch, FileData, SheetDeletedPatch, SheetInsertedPatch};

pub(crate) enum CurrentStructureShape {
    Empty,
    Sheets {
        sheet_index: usize,
        sheet_count: usize,
    },
}

impl CurrentStructureShape {
    pub(crate) fn capture(file_data: &FileData, target: &FileStructureMemento) -> Self {
        match target {
            FileStructureMemento::Empty { .. } => Self::Empty,
            FileStructureMemento::Row(_) | FileStructureMemento::Column(_) => Self::Empty,
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
        (_, FileStructureMemento::Row(target)) => sheet_updated_patch(restored, target.sheet_index),
        (_, FileStructureMemento::Column(target)) => {
            sheet_updated_patch(restored, target.sheet_index)
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
