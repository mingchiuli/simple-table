//! Memento capture and restore for [`SpreadsheetDocument`].
//!
//! These are inherent methods, so callers keep using `SpreadsheetDocument`
//! directly. The module is a child of the document model, which lets it reach
//! private fields without widening their visibility.

use super::*;

impl SpreadsheetDocument {
    pub fn create_memento(
        before: DocumentMementoSide,
        after: DocumentMementoSide,
    ) -> DocumentMemento {
        DocumentMemento::new(before, after)
    }

    pub fn capture_memento_side(
        &mut self,
        operation: &AppliedOperation,
        after_operation: bool,
    ) -> DocumentMementoSide {
        match operation {
            AppliedOperation::SetCell {
                sheet_index,
                row,
                col,
                ..
            } => DocumentMementoSide::Cells(self.cell_memento(
                [FormulaCellRef {
                    sheet_index: *sheet_index,
                    row: *row,
                    col: *col,
                }],
                operation_may_change_formula_capabilities(operation),
            )),
            AppliedOperation::SetCells { changes } => {
                DocumentMementoSide::Cells(self.cell_batch_memento(
                    changes,
                    operation_may_change_formula_capabilities(operation),
                ))
            }
            AppliedOperation::SetColumnWidth {
                sheet_index,
                col_index,
                ..
            } => DocumentMementoSide::Layout(self.layout_memento(
                *sheet_index,
                Some(*col_index),
                None,
            )),
            AppliedOperation::SetRowHeight {
                sheet_index,
                row_index,
                ..
            } => DocumentMementoSide::Layout(self.layout_memento(
                *sheet_index,
                None,
                Some(*row_index),
            )),
            AppliedOperation::AddRow { .. }
            | AppliedOperation::DeleteRow { .. }
            | AppliedOperation::AddColumn { .. }
            | AppliedOperation::DeleteColumn { .. } => {
                DocumentMementoSide::Structure(Box::new(self.structure_memento(operation)))
            }
            AppliedOperation::AddSheet { .. } | AppliedOperation::DeleteSheet { .. } => {
                DocumentMementoSide::Structure(Box::new(self.structure_memento(operation)))
            }
            AppliedOperation::InsertImage {
                sheet_index,
                image,
                bytes,
                image_name,
                ..
            } => {
                let current_image = self.current_image(*sheet_index, &image.id);
                DocumentMementoSide::Image(ImageMemento {
                    sheet_index: *sheet_index,
                    image_id: image.id.clone(),
                    asset: current_image.as_ref().map(|_| {
                        crate::document::backing::document_body::BodyImageAsset {
                            image_name: image_name.clone(),
                            bytes: bytes.clone(),
                        }
                    }),
                    image: current_image,
                })
            }
            AppliedOperation::UpdateImage {
                sheet_index,
                new_image,
                ..
            } => DocumentMementoSide::Image(ImageMemento {
                sheet_index: *sheet_index,
                image_id: new_image.id.clone(),
                image: self.current_image(*sheet_index, &new_image.id),
                asset: None,
            }),
            AppliedOperation::DeleteImage {
                sheet_index, image, ..
            } => DocumentMementoSide::Image(ImageMemento {
                sheet_index: *sheet_index,
                image_id: image.id.clone(),
                image: self.current_image(*sheet_index, &image.id),
                asset: self
                    .current_image(*sheet_index, &image.id)
                    .and_then(|current| self.body.image_asset(*sheet_index, current.z_index)),
            }),
            AppliedOperation::SortRows(sort) => {
                DocumentMementoSide::Sort(crate::document::document_memento::SortMemento {
                    sheet_index: sort.sheet_index,
                    range: sort.range,
                    permutation: if after_operation {
                        sort.permutation.clone()
                    } else {
                        sort.inverse_permutation.clone()
                    },
                    formulas: if after_operation {
                        sort.after_formulas.clone()
                    } else {
                        sort.before_formulas.clone()
                    },
                })
            }
        }
    }

    pub(crate) fn estimate_memento_side_bytes(&mut self, operation: &AppliedOperation) -> usize {
        document_memento_budget::estimate_memento_side_bytes(
            &self.projection,
            &self.body,
            &mut self.formulas,
            operation,
        )
    }

    pub(crate) fn restore_memento_side(
        &mut self,
        side: &DocumentMementoSide,
    ) -> Result<DocumentRestoreResult, AppError> {
        match side {
            DocumentMementoSide::Cells(memento) => self.restore_cells(memento),
            DocumentMementoSide::Layout(memento) => self.restore_layout(memento),
            DocumentMementoSide::Structure(memento) => self.restore_structure(memento),
            DocumentMementoSide::Image(memento) => self.restore_image(memento),
            DocumentMementoSide::Sort(memento) => self.restore_sort(memento),
        }
    }

    pub(super) fn restore_sort(
        &mut self,
        memento: &crate::document::document_memento::SortMemento,
    ) -> Result<DocumentRestoreResult, AppError> {
        let operation = AppliedOperation::SortRows(crate::domain::ResolvedSort {
            sheet_index: memento.sheet_index,
            range: memento.range,
            permutation: memento.permutation.clone(),
            inverse_permutation: Vec::new(),
            before_formulas: Vec::new(),
            after_formulas: memento.formulas.clone(),
        });
        self.apply_operation_to_body_and_projection(&operation)?;
        let formula_changes = self.recalculate_after_operation(&operation);
        self.patch_workbook_formula_changes(&formula_changes)?;
        self.fail_post_patch_restore_if_injected()?;
        self.validate_persisted_projection_consistency()?;
        self.validate_projection_sheets([memento.sheet_index])?;
        self.refresh_region_metadata_index();
        Ok(DocumentRestoreResult {
            changes: vec![DocumentRestoreChange::SheetInvalidated {
                sheet_index: memento.sheet_index,
            }],
        })
    }

    pub(super) fn current_image(
        &self,
        sheet_index: usize,
        image_id: &str,
    ) -> Option<crate::document::data::SheetImage> {
        self.projection
            .sheets
            .get(sheet_index)?
            .rich
            .images
            .iter()
            .find(|image| image.id == image_id)
            .cloned()
    }

    pub(super) fn restore_image(
        &mut self,
        memento: &ImageMemento,
    ) -> Result<DocumentRestoreResult, AppError> {
        let current = self.current_image(memento.sheet_index, &memento.image_id);
        let operation = match (&current, &memento.image) {
            (Some(old_image), Some(new_image)) => AppliedOperation::UpdateImage {
                sheet_index: memento.sheet_index,
                old_image: old_image.clone(),
                new_image: new_image.clone(),
            },
            (None, Some(image)) => {
                let asset = memento.asset.as_ref().ok_or_else(|| {
                    AppError::DocumentStateInvalid(format!(
                        "image {} cannot be restored without media data",
                        memento.image_id
                    ))
                })?;
                AppliedOperation::InsertImage {
                    sheet_index: memento.sheet_index,
                    image: image.clone(),
                    image_name: asset.image_name.clone(),
                    bytes: asset.bytes.clone(),
                    // Undo never reverts the layout written by the insert, so
                    // redo must not touch the row/column sizes either.
                    column_width: None,
                    row_height: None,
                }
            }
            (Some(image), None) => AppliedOperation::DeleteImage {
                sheet_index: memento.sheet_index,
                image: image.clone(),
            },
            (None, None) => return Ok(DocumentRestoreResult::default()),
        };

        operation
            .projection_mutation()
            .execute(&mut self.projection);
        self.body
            .patch_after_operation(&mut self.projection, &operation, &[])?;
        self.fail_post_patch_restore_if_injected()?;
        self.validate_projection_sheets([memento.sheet_index])?;
        let change = match &memento.image {
            Some(image) => DocumentRestoreChange::ImageUpserted {
                sheet_index: memento.sheet_index,
                image: image.clone(),
            },
            None => DocumentRestoreChange::ImageDeleted {
                sheet_index: memento.sheet_index,
                image_id: memento.image_id.clone(),
            },
        };
        Ok(DocumentRestoreResult {
            changes: vec![change],
        })
    }

    pub(super) fn fail_restore_if_injected(&mut self) -> Result<(), AppError> {
        #[cfg(test)]
        if self.injected_restore_failures > 0 {
            self.injected_restore_failures -= 1;
            return Err(AppError::WorkbookPatchFailed(
                "injected history restore failure".to_string(),
            ));
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn inject_restore_failures(&mut self, count: usize) {
        self.injected_restore_failures = count;
    }

    pub(super) fn fail_post_patch_restore_if_injected(&mut self) -> Result<(), AppError> {
        #[cfg(test)]
        if self.injected_post_patch_restore_failures > 0 {
            self.injected_post_patch_restore_failures -= 1;
            return Err(AppError::WorkbookPatchFailed(
                "injected post-patch history restore failure".to_string(),
            ));
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn inject_post_patch_restore_failures(&mut self, count: usize) {
        self.injected_post_patch_restore_failures = count;
    }

    pub(super) fn cell_memento(
        &self,
        changed_cells: impl IntoIterator<Item = FormulaCellRef>,
        formula_capabilities_may_change: bool,
    ) -> CellMemento {
        let changed_cells: Vec<FormulaCellRef> = changed_cells.into_iter().collect();
        let formula_cells = self
            .formulas
            .impacted_cells_for_memento(changed_cells.iter().copied(), &self.projection);
        self.cell_positions_memento(
            changed_cells
                .into_iter()
                .chain(formula_cells)
                .map(|cell| (cell.sheet_index, cell.row, cell.col)),
            formula_capabilities_may_change,
        )
    }

    pub(super) fn cell_batch_memento(
        &self,
        changes: &[ResolvedCellEdit],
        formula_capabilities_may_change: bool,
    ) -> CellMemento {
        self.cell_memento(
            changes.iter().map(|change| FormulaCellRef {
                sheet_index: change.sheet_index,
                row: change.row,
                col: change.col,
            }),
            formula_capabilities_may_change,
        )
    }

    pub(super) fn cell_positions_memento(
        &self,
        positions_to_capture: impl IntoIterator<Item = (usize, usize, usize)>,
        formula_capabilities_may_change: bool,
    ) -> CellMemento {
        let mut positions = Vec::new();
        let mut seen = HashSet::new();
        for (sheet_index, row, col) in positions_to_capture {
            push_unique_position(&mut positions, &mut seen, sheet_index, row, col);
        }
        let sheet_shapes = positions
            .iter()
            .map(|(sheet_index, _, _)| *sheet_index)
            .collect::<HashSet<_>>()
            .into_iter()
            .map(|sheet_index| SheetShapeMemento {
                sheet_index,
                row_lengths: self
                    .projection
                    .sheets
                    .get(sheet_index)
                    .map(|sheet| sheet.rows.iter().map(Vec::len).collect())
                    .unwrap_or_default(),
                protected_cells: self
                    .projection
                    .sheets
                    .get(sheet_index)
                    .map(protected_rich_cell_positions)
                    .unwrap_or_default(),
            })
            .collect();

        let cells = positions
            .into_iter()
            .map(|(sheet_index, row, col)| {
                DocumentCellChange::new(
                    sheet_index,
                    row,
                    col,
                    self.projection_cell(sheet_index, row, col),
                )
            })
            .collect();

        CellMemento::new(cells, sheet_shapes, formula_capabilities_may_change)
    }

    pub(super) fn layout_memento(
        &self,
        sheet_index: usize,
        col_index: Option<usize>,
        row_index: Option<usize>,
    ) -> LayoutMemento {
        let mut column_widths = HashMap::new();
        let mut row_heights = HashMap::new();
        if let Some(col_index) = col_index {
            column_widths.insert(
                col_index,
                self.projection
                    .sheets
                    .get(sheet_index)
                    .and_then(|sheet| sheet.column_widths.as_ref())
                    .and_then(|widths| widths.get(&col_index).copied()),
            );
        }
        if let Some(row_index) = row_index {
            row_heights.insert(
                row_index,
                self.projection
                    .sheets
                    .get(sheet_index)
                    .and_then(|sheet| sheet.row_heights.as_ref())
                    .and_then(|heights| heights.get(&row_index).copied()),
            );
        }

        LayoutMemento::new(sheet_index, column_widths, row_heights)
    }

    pub(super) fn structure_memento(&mut self, operation: &AppliedOperation) -> StructureMemento {
        let formula_sheet_indexes = self
            .formulas
            .structure_memento_sheet_indexes(&self.projection, operation);
        let body = self
            .body
            .capture_structure_memento(operation, formula_sheet_indexes);
        let projection = self.projection_structure_memento(operation);

        StructureMemento::new(projection, body)
    }

    pub(super) fn projection_structure_memento(
        &self,
        operation: &AppliedOperation,
    ) -> FileStructureMemento {
        let sheet_count = self.projection.sheets.len();
        match operation {
            AppliedOperation::AddRow {
                sheet_index,
                row_index,
                ..
            }
            | AppliedOperation::DeleteRow {
                sheet_index,
                row_index,
            } => self
                .projection
                .sheets
                .get(*sheet_index)
                .map(|sheet| {
                    FileStructureMemento::Row(RowStructureMemento {
                        sheet_index: *sheet_index,
                        row_index: *row_index,
                        row_count: sheet.rows.len(),
                        row: sheet.rows.get(*row_index).cloned(),
                        merges: sheet.merges.clone(),
                        row_heights: sheet.row_heights.clone(),
                        rich: RichProjectionMemento::row_tail(&sheet.rich, *row_index),
                    })
                })
                .unwrap_or_else(|| FileStructureMemento::empty(sheet_count)),
            AppliedOperation::AddColumn {
                sheet_index,
                col_index,
                ..
            }
            | AppliedOperation::DeleteColumn {
                sheet_index,
                col_index,
            } => self
                .projection
                .sheets
                .get(*sheet_index)
                .map(|sheet| {
                    FileStructureMemento::Column(ColumnStructureMemento {
                        sheet_index: *sheet_index,
                        col_index: *col_index,
                        row_lengths: sheet.rows.iter().map(Vec::len).collect(),
                        values: sheet
                            .rows
                            .iter()
                            .map(|row| row.get(*col_index).cloned())
                            .collect(),
                        merges: sheet.merges.clone(),
                        column_widths: sheet.column_widths.clone(),
                        rich: RichProjectionMemento::column_tail(&sheet.rich, *col_index),
                    })
                })
                .unwrap_or_else(|| FileStructureMemento::empty(sheet_count)),
            AppliedOperation::AddSheet { sheet_index, .. }
            | AppliedOperation::DeleteSheet { sheet_index } => {
                FileStructureMemento::Sheets(SheetTailMemento {
                    sheet_count,
                    truncate_from: *sheet_index,
                    sheets: (*sheet_index..sheet_count)
                        .filter_map(|sheet_index| {
                            self.projection
                                .sheets
                                .get(sheet_index)
                                .cloned()
                                .map(|sheet| ProjectionSheetSnapshot { sheet_index, sheet })
                        })
                        .collect(),
                })
            }
            AppliedOperation::SetCell { .. }
            | AppliedOperation::SetCells { .. }
            | AppliedOperation::SetColumnWidth { .. }
            | AppliedOperation::SetRowHeight { .. }
            | AppliedOperation::InsertImage { .. }
            | AppliedOperation::UpdateImage { .. }
            | AppliedOperation::DeleteImage { .. }
            | AppliedOperation::SortRows(_) => FileStructureMemento::empty(sheet_count),
        }
    }

    pub(super) fn projection_cell(&self, sheet_index: usize, row: usize, col: usize) -> CellValue {
        self.projection
            .sheets
            .get(sheet_index)
            .and_then(|sheet| sheet.rows.get(row))
            .and_then(|row_data| row_data.get(col))
            .cloned()
            .unwrap_or(CellValue::Null)
    }

    pub(super) fn restore_cells(
        &mut self,
        memento: &CellMemento,
    ) -> Result<DocumentRestoreResult, AppError> {
        for change in &memento.cells {
            let Some(sheet) = self.projection.sheets.get_mut(change.sheet_index) else {
                continue;
            };
            ensure_projection_cell_exists(sheet, change.row, change.col);
            sheet.rows[change.row][change.col] = change.value.clone();
        }

        self.fail_restore_if_injected()?;
        self.patch_workbook_formula_changes(&memento.cells)?;
        self.restore_cell_shapes(&memento.sheet_shapes);
        self.patch_workbook_cell_shapes(&memento.sheet_shapes)?;
        let formula_changes = self.formulas.rebuild(&mut self.projection);
        if !formula_changes.is_empty() {
            self.patch_workbook_formula_changes(&formula_changes)?;
        }
        if memento.formula_capabilities_may_change {
            self.refresh_capabilities();
        }
        self.fail_post_patch_restore_if_injected()?;
        self.validate_projection_sheets(cell_memento_sheet_indexes(memento))?;
        let mut changes = Vec::new();
        let mut cell_changes = memento.cells.clone();
        for change in formula_changes {
            push_sheet_cell_change_if_missing(&mut cell_changes, change);
        }
        if !cell_changes.is_empty() {
            changes.push(DocumentRestoreChange::Cells(cell_changes));
        }
        changes.extend(shape_restore_changes(&memento.sheet_shapes));
        Ok(DocumentRestoreResult { changes })
    }

    pub(super) fn restore_cell_shapes(&mut self, shapes: &[SheetShapeMemento]) {
        for shape in shapes {
            let Some(sheet) = self.projection.sheets.get_mut(shape.sheet_index) else {
                continue;
            };
            sheet.rows.truncate(shape.row_lengths.len());
            for (row, len) in shape.row_lengths.iter().copied().enumerate() {
                if let Some(row_data) = sheet.rows.get_mut(row) {
                    row_data.truncate(len);
                }
            }
        }
    }

    pub(super) fn restore_layout(
        &mut self,
        memento: &LayoutMemento,
    ) -> Result<DocumentRestoreResult, AppError> {
        let Some(sheet) = self.projection.sheets.get_mut(memento.sheet_index) else {
            return Ok(DocumentRestoreResult::default());
        };

        for (col_index, width) in &memento.column_widths {
            match width {
                Some(width) => {
                    sheet
                        .column_widths
                        .get_or_insert_with(Default::default)
                        .insert(*col_index, *width);
                }
                None => {
                    if let Some(widths) = sheet.column_widths.as_mut() {
                        widths.remove(col_index);
                        if widths.is_empty() {
                            sheet.column_widths = None;
                        }
                    }
                }
            }
        }

        for (row_index, height) in &memento.row_heights {
            match height {
                Some(height) => {
                    sheet
                        .row_heights
                        .get_or_insert_with(Default::default)
                        .insert(*row_index, *height);
                }
                None => {
                    if let Some(heights) = sheet.row_heights.as_mut() {
                        heights.remove(row_index);
                        if heights.is_empty() {
                            sheet.row_heights = None;
                        }
                    }
                }
            }
        }

        self.fail_restore_if_injected()?;
        self.patch_workbook_layout(memento)?;
        self.fail_post_patch_restore_if_injected()?;
        self.validate_projection_sheets([memento.sheet_index])?;
        Ok(DocumentRestoreResult {
            changes: vec![DocumentRestoreChange::Layout {
                sheet_index: memento.sheet_index,
                column_widths: memento.column_widths.clone(),
                row_heights: memento.row_heights.clone(),
            }],
        })
    }

    pub(super) fn restore_structure(
        &mut self,
        memento: &StructureMemento,
    ) -> Result<DocumentRestoreResult, AppError> {
        let current_shape = CurrentStructureShape::capture(&self.projection, &memento.projection);
        match self.body.restore_structure_memento(&memento.body)? {
            BodyRestoreAction::RefreshProjectionFromWorkbook => {
                self.refresh_projection_from_workbook();
            }
            BodyRestoreAction::RestoreProjectionOnly => {
                restore_projection_structure(&mut self.projection, &memento.projection);
            }
        }
        self.fail_restore_if_injected()?;
        self.refresh_capabilities();
        let formula_changes = self.formulas.rebuild(&mut self.projection);
        if !formula_changes.is_empty() {
            self.patch_workbook_formula_changes(&formula_changes)?;
        }
        self.fail_post_patch_restore_if_injected()?;
        self.validate_persisted_projection_consistency()?;
        self.validate_projection_consistency()?;
        let mut changes =
            restore_structure_changes(&current_shape, &memento.projection, &self.projection);
        if changes.is_empty() {
            changes.push(DocumentRestoreChange::ResyncRequired {
                reason: "structure restore changed workbook projection".to_string(),
            });
        }
        if !formula_changes.is_empty() {
            changes.push(DocumentRestoreChange::Cells(formula_changes));
        }
        self.refresh_region_metadata_index();
        Ok(DocumentRestoreResult { changes })
    }
}

fn push_unique_position(
    positions: &mut Vec<(usize, usize, usize)>,
    seen: &mut HashSet<(usize, usize, usize)>,
    sheet_index: usize,
    row: usize,
    col: usize,
) {
    if seen.insert((sheet_index, row, col)) {
        positions.push((sheet_index, row, col));
    }
}
