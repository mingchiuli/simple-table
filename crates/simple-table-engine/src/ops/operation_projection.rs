use crate::document_data::DocumentData;
use crate::document_data::SheetImage;
use crate::domain::{AppliedOperation, CellValue, OperationPatchProjector};
use crate::projection_model::{ProjectedCellChange, SheetLayoutSnapshot, SheetManifestSnapshot};

#[derive(Clone, Debug)]
pub enum ProjectedOperation {
    SetCell {
        sheet_index: usize,
        row: usize,
        col: usize,
        value: CellValue,
    },
    SetCells {
        changes: Vec<ProjectedCellChange>,
    },
    AddRow {
        sheet_index: usize,
        row_index: usize,
    },
    DeleteRow {
        sheet_index: usize,
        row_index: usize,
    },
    AddColumn {
        sheet_index: usize,
        col_index: usize,
    },
    DeleteColumn {
        sheet_index: usize,
        column_index: usize,
    },
    SetColumnWidth,
    SetRowHeight,
    AddSheet {
        sheet_index: usize,
        sheet: SheetManifestSnapshot,
    },
    DeleteSheet {
        sheet_index: usize,
    },
    ImageUpserted {
        sheet_index: usize,
        image: SheetImage,
    },
    ImageDeleted {
        sheet_index: usize,
        image_id: String,
    },
    SortRows {
        sheet_index: usize,
    },
}

impl OperationPatchProjector<'_> {
    pub fn projected_result_from_current_file(
        &self,
        file_data: &DocumentData,
    ) -> ProjectedOperation {
        match self.operation {
            AppliedOperation::SetCell {
                sheet_index,
                row,
                col,
                new_value,
                ..
            } => ProjectedOperation::SetCell {
                sheet_index: *sheet_index,
                row: *row,
                col: *col,
                value: file_data
                    .sheets
                    .get(*sheet_index)
                    .and_then(|sheet| sheet.rows.get(*row))
                    .and_then(|row_data| row_data.get(*col))
                    .cloned()
                    .unwrap_or_else(|| new_value.clone()),
            },
            AppliedOperation::SetCells { changes } => ProjectedOperation::SetCells {
                changes: changes
                    .iter()
                    .map(|change| {
                        ProjectedCellChange::new(
                            change.sheet_index,
                            change.row,
                            change.col,
                            file_data
                                .sheets
                                .get(change.sheet_index)
                                .and_then(|sheet| sheet.rows.get(change.row))
                                .and_then(|row| row.get(change.col))
                                .cloned()
                                .unwrap_or_else(|| change.new_value.clone()),
                        )
                    })
                    .collect(),
            },
            AppliedOperation::SetColumnWidth { .. } => ProjectedOperation::SetColumnWidth,
            AppliedOperation::SetRowHeight { .. } => ProjectedOperation::SetRowHeight,
            AppliedOperation::AddRow {
                sheet_index,
                row_index,
                ..
            } => ProjectedOperation::AddRow {
                sheet_index: *sheet_index,
                row_index: *row_index,
            },
            AppliedOperation::DeleteRow {
                sheet_index,
                row_index,
            } => ProjectedOperation::DeleteRow {
                sheet_index: *sheet_index,
                row_index: *row_index,
            },
            AppliedOperation::AddColumn {
                sheet_index,
                col_index,
                ..
            } => ProjectedOperation::AddColumn {
                sheet_index: *sheet_index,
                col_index: *col_index,
            },
            AppliedOperation::DeleteColumn {
                sheet_index,
                col_index,
            } => ProjectedOperation::DeleteColumn {
                sheet_index: *sheet_index,
                column_index: *col_index,
            },
            AppliedOperation::AddSheet {
                sheet_index,
                name,
                row_count,
                column_count,
            } => ProjectedOperation::AddSheet {
                sheet_index: *sheet_index,
                sheet: file_data
                    .sheets
                    .get(*sheet_index)
                    .map(|sheet| SheetManifestSnapshot {
                        name: sheet.name.clone(),
                        extent: sheet.extent(),
                        layout: SheetLayoutSnapshot {
                            column_widths: sheet.column_widths.clone().unwrap_or_default(),
                            row_heights: sheet.row_heights.clone().unwrap_or_default(),
                        },
                    })
                    .unwrap_or_else(|| SheetManifestSnapshot {
                        name: name.clone(),
                        extent: crate::document_data::SheetExtent {
                            row_count: *row_count,
                            column_count: *column_count,
                        },
                        layout: SheetLayoutSnapshot::default(),
                    }),
            },
            AppliedOperation::DeleteSheet { sheet_index } => ProjectedOperation::DeleteSheet {
                sheet_index: *sheet_index,
            },
            AppliedOperation::InsertImage {
                sheet_index, image, ..
            } => ProjectedOperation::ImageUpserted {
                sheet_index: *sheet_index,
                image: file_data
                    .sheets
                    .get(*sheet_index)
                    .and_then(|sheet| sheet.rich.images.iter().find(|item| item.id == image.id))
                    .cloned()
                    .unwrap_or_else(|| image.clone()),
            },
            AppliedOperation::UpdateImage {
                sheet_index,
                new_image,
                ..
            } => ProjectedOperation::ImageUpserted {
                sheet_index: *sheet_index,
                image: file_data
                    .sheets
                    .get(*sheet_index)
                    .and_then(|sheet| {
                        sheet
                            .rich
                            .images
                            .iter()
                            .find(|item| item.id == new_image.id)
                    })
                    .cloned()
                    .unwrap_or_else(|| new_image.clone()),
            },
            AppliedOperation::DeleteImage {
                sheet_index, image, ..
            } => ProjectedOperation::ImageDeleted {
                sheet_index: *sheet_index,
                image_id: image.id.clone(),
            },
            AppliedOperation::SortRows(sort) => ProjectedOperation::SortRows {
                sheet_index: sort.sheet_index,
            },
        }
    }
}
