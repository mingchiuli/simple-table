use crate::io::document_memento::FileStructureMemento;
use crate::ops::patch_projector::sheet_invalidated_patch;
use crate::types::{EditorPatch, FileData, SheetManifest, SheetsReplacedPatch};

pub(crate) enum CurrentStructureShape {
    Empty,
    Sheets { sheet_index: usize },
}

impl CurrentStructureShape {
    pub(crate) fn capture(_file_data: &FileData, target: &FileStructureMemento) -> Self {
        match target {
            FileStructureMemento::Empty { .. } => Self::Empty,
            FileStructureMemento::Row(_) | FileStructureMemento::Column(_) => Self::Empty,
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
        (_, FileStructureMemento::Row(target)) => sheet_invalidated_patch(target.sheet_index),
        (_, FileStructureMemento::Column(target)) => sheet_invalidated_patch(target.sheet_index),
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
                        })
                        .collect(),
                },
            }]
        }
        _ => Vec::new(),
    }
}
