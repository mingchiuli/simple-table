use crate::document_data::{DocumentData, DocumentSheet, ImageAnchor, ImageMarker};
use std::collections::HashMap;

use crate::document::backing::workbook_patch::{StructurePatchDiagnostics, WorkbookSheetShape};
use crate::document::backing::workbook_port::WorkbookBackingPort;
use crate::document::capabilities::{
    SheetCapabilities, WorkbookCapabilities, WorkbookImageCapabilities, WorkbookRichCapabilities,
    WorkbookSaveCapabilities, WorkbookStructureCapabilities,
};
use crate::domain::{AppliedOperation, CellValue, DocumentCellChange};
use crate::error::AppError;
use crate::formula::ast::FormulaAstService;
use crate::formula::reference_rewrite::{
    StructureShift, adjust_explicit_sheet_name_case_mismatched_references,
    adjust_formula_references, invalidate_deleted_sheet_references,
};
use umya_spreadsheet::{Workbook, Worksheet};

#[derive(Clone, Copy)]
enum FormulaRewriteScope {
    CrossSheetOnly,
}

#[derive(Clone, Copy)]
struct FormulaRewritePlan<'a> {
    target_sheet_name: &'a str,
    shift: StructureShift,
    scope: FormulaRewriteScope,
}

pub fn patch_after_operation(
    workbook: &mut Workbook,
    file_data: &mut DocumentData,
    operation: &AppliedOperation,
    cell_changes: &[DocumentCellChange],
    backing: &dyn WorkbookBackingPort,
) -> Result<(), AppError> {
    match operation {
        AppliedOperation::SetCell {
            sheet_index,
            row,
            col,
            ..
        } => {
            patch_cell(workbook, file_data, *sheet_index, *row, *col, backing)?;
            patch_cell_changes(workbook, file_data, cell_changes, backing)?;
        }
        AppliedOperation::SetCells { changes } => {
            for change in changes {
                patch_cell(
                    workbook,
                    file_data,
                    change.sheet_index,
                    change.row,
                    change.col,
                    backing,
                )?;
            }
            patch_cell_changes(workbook, file_data, cell_changes, backing)?;
        }
        AppliedOperation::AddRow { .. }
        | AppliedOperation::DeleteRow { .. }
        | AppliedOperation::AddColumn { .. }
        | AppliedOperation::DeleteColumn { .. }
        | AppliedOperation::AddSheet { .. }
        | AppliedOperation::DeleteSheet { .. } => {}
        AppliedOperation::SetColumnWidth {
            sheet_index,
            col_index,
            new_width,
            ..
        } => {
            if let Some(worksheet) = sheet_mut(workbook, *sheet_index)? {
                patch_column_width(worksheet, *col_index, *new_width, backing);
            }
        }
        AppliedOperation::SetRowHeight {
            sheet_index,
            row_index,
            new_height,
            ..
        } => {
            if let Some(worksheet) = sheet_mut(workbook, *sheet_index)? {
                patch_row_height(worksheet, *row_index, *new_height, backing);
            }
        }
        AppliedOperation::InsertImage {
            sheet_index,
            image,
            image_name,
            bytes,
            column_width,
            row_height,
        } => {
            if let Some(worksheet) = sheet_mut(workbook, *sheet_index)? {
                let marker = marker_type(match &image.anchor {
                    ImageAnchor::OneCell { from, .. } | ImageAnchor::TwoCell { from, .. } => from,
                });
                let mut workbook_image = umya_spreadsheet::Image::default();
                workbook_image.new_image_with_dimensions(
                    image.intrinsic_height,
                    image.intrinsic_width,
                    image_name,
                    bytes.as_ref().to_vec(),
                    marker,
                );
                set_image_identity(&mut workbook_image, &image.id);
                apply_image_anchor(&mut workbook_image, &image.anchor)?;
                worksheet.add_image(workbook_image);
                if let ImageAnchor::OneCell { from, .. } = &image.anchor {
                    if let Some(width) = column_width {
                        patch_column_width(worksheet, from.col as usize, Some(*width), backing);
                    }
                    if let Some(height) = row_height {
                        patch_row_height(worksheet, from.row as usize, Some(*height), backing);
                    }
                }
            }
        }
        AppliedOperation::UpdateImage {
            sheet_index,
            old_image,
            new_image,
        } => {
            if let Some(worksheet) = sheet_mut(workbook, *sheet_index)? {
                let workbook_image = worksheet
                    .image_collection_mut()
                    .get_mut(old_image.z_index)
                    .ok_or_else(|| {
                        AppError::WorkbookPatchFailed(format!(
                            "workbook image {} is missing",
                            old_image.id
                        ))
                    })?;
                apply_image_anchor(workbook_image, &new_image.anchor)?;
            }
        }
        AppliedOperation::DeleteImage {
            sheet_index, image, ..
        } => {
            if let Some(worksheet) = sheet_mut(workbook, *sheet_index)? {
                if image.z_index >= worksheet.image_collection().len() {
                    return Err(AppError::WorkbookPatchFailed(format!(
                        "workbook image {} is missing",
                        image.id
                    )));
                }
                worksheet.image_collection_mut().remove(image.z_index);
            }
        }
        AppliedOperation::SortRows(_) => {}
    }

    Ok(())
}

pub fn apply_structure_operation(
    workbook: &mut Workbook,
    operation: &AppliedOperation,
    ast_service: &mut FormulaAstService,
    backing: &dyn WorkbookBackingPort,
) -> Result<StructurePatchDiagnostics, AppError> {
    let unsupported = unsupported_operation_features(workbook, operation, &[]);
    if !unsupported.is_empty() {
        return Err(AppError::UnsupportedWorkbookStructure(
            unsupported.join(", "),
        ));
    }

    let mut diagnostics = StructurePatchDiagnostics::default();

    match operation {
        AppliedOperation::AddRow {
            sheet_index,
            row_index,
            ..
        } => {
            let sheet_name = sheet_name(workbook, *sheet_index)?;
            if let Some(worksheet) = sheet_mut(workbook, *sheet_index)? {
                worksheet.insert_new_row(*row_index as u32 + 1, 1);
            }
            diagnostics.skipped_formula_reference_rewrites +=
                rewrite_formulas_after_native_structure_shift(
                    workbook,
                    ast_service,
                    FormulaRewritePlan {
                        target_sheet_name: &sheet_name,
                        shift: StructureShift::InsertRows {
                            row_index: *row_index,
                            count: 1,
                        },
                        scope: FormulaRewriteScope::CrossSheetOnly,
                    },
                );
        }
        AppliedOperation::DeleteRow {
            sheet_index,
            row_index,
        } => {
            let sheet_name = sheet_name(workbook, *sheet_index)?;
            if let Some(worksheet) = sheet_mut(workbook, *sheet_index)? {
                worksheet.remove_row(*row_index as u32 + 1, 1);
            }
            diagnostics.skipped_formula_reference_rewrites +=
                rewrite_formulas_after_native_structure_shift(
                    workbook,
                    ast_service,
                    FormulaRewritePlan {
                        target_sheet_name: &sheet_name,
                        shift: StructureShift::DeleteRows {
                            row_index: *row_index,
                            count: 1,
                        },
                        scope: FormulaRewriteScope::CrossSheetOnly,
                    },
                );
        }
        AppliedOperation::AddColumn {
            sheet_index,
            col_index,
            ..
        } => {
            let sheet_name = sheet_name(workbook, *sheet_index)?;
            if let Some(worksheet) = sheet_mut(workbook, *sheet_index)? {
                worksheet.insert_new_column_by_index(*col_index as u32 + 1, 1);
            }
            diagnostics.skipped_formula_reference_rewrites +=
                rewrite_formulas_after_native_structure_shift(
                    workbook,
                    ast_service,
                    FormulaRewritePlan {
                        target_sheet_name: &sheet_name,
                        shift: StructureShift::InsertColumns {
                            col_index: *col_index,
                            count: 1,
                        },
                        scope: FormulaRewriteScope::CrossSheetOnly,
                    },
                );
        }
        AppliedOperation::DeleteColumn {
            sheet_index,
            col_index,
        } => {
            let sheet_name = sheet_name(workbook, *sheet_index)?;
            if let Some(worksheet) = sheet_mut(workbook, *sheet_index)? {
                worksheet.remove_column_by_index(*col_index as u32 + 1, 1);
            }
            diagnostics.skipped_formula_reference_rewrites +=
                rewrite_formulas_after_native_structure_shift(
                    workbook,
                    ast_service,
                    FormulaRewritePlan {
                        target_sheet_name: &sheet_name,
                        shift: StructureShift::DeleteColumns {
                            col_index: *col_index,
                            count: 1,
                        },
                        scope: FormulaRewriteScope::CrossSheetOnly,
                    },
                );
        }
        AppliedOperation::AddSheet {
            sheet_index,
            name,
            row_count,
            column_count,
        } => {
            let sheet_data = DocumentSheet {
                name: name.clone(),
                rows: vec![vec![CellValue::Null; *column_count]; *row_count],
                ..Default::default()
            };
            insert_sheet(workbook, *sheet_index, &sheet_data, backing)?;
        }
        AppliedOperation::DeleteSheet { sheet_index } => {
            diagnostics.skipped_formula_reference_rewrites +=
                invalidate_sheet_references_before_delete(workbook, ast_service, *sheet_index)?;
            remove_sheet(workbook, *sheet_index)?;
        }
        AppliedOperation::SortRows(sort) => {
            if let Some(worksheet) = sheet_mut(workbook, sort.sheet_index)? {
                apply_native_row_permutation(
                    worksheet,
                    sort.range,
                    &sort.permutation,
                    &sort.after_formulas,
                    backing,
                )?;
            }
        }
        AppliedOperation::SetCell { .. }
        | AppliedOperation::SetCells { .. }
        | AppliedOperation::SetColumnWidth { .. }
        | AppliedOperation::SetRowHeight { .. }
        | AppliedOperation::InsertImage { .. }
        | AppliedOperation::UpdateImage { .. }
        | AppliedOperation::DeleteImage { .. } => {}
    }

    Ok(diagnostics)
}

fn apply_native_row_permutation(
    worksheet: &mut Worksheet,
    range: crate::domain::CellRange,
    permutation: &[usize],
    formulas: &[crate::domain::FormulaTextAtCell],
    backing: &dyn WorkbookBackingPort,
) -> Result<(), AppError> {
    let body_start = range.body_start_row();
    let mut visited = vec![false; permutation.len()];
    for start in 0..permutation.len() {
        if visited[start] || permutation[start] == start {
            visited[start] = true;
            continue;
        }
        let saved = take_native_row_segment(
            worksheet,
            body_start + start,
            range.start_col,
            range.end_col,
        );
        let mut destination = start;
        loop {
            visited[destination] = true;
            let source = permutation[destination];
            if source == start {
                put_native_row_segment(worksheet, body_start + destination, range.start_col, saved);
                break;
            }
            let cells = take_native_row_segment(
                worksheet,
                body_start + source,
                range.start_col,
                range.end_col,
            );
            put_native_row_segment(worksheet, body_start + destination, range.start_col, cells);
            destination = source;
        }
    }
    for formula in formulas {
        backing.write_cell(
            worksheet,
            formula.row as u32 + 1,
            formula.col as u32 + 1,
            &CellValue::formula(&formula.formula, CellValue::Null),
        );
    }
    Ok(())
}

fn take_native_row_segment(
    worksheet: &mut Worksheet,
    row: usize,
    start_col: usize,
    end_col: usize,
) -> Vec<Option<umya_spreadsheet::Cell>> {
    (start_col..=end_col)
        .map(|col| {
            let coordinate = (col as u32 + 1, row as u32 + 1);
            let cell = worksheet.cell(coordinate).cloned();
            if cell.is_some() {
                worksheet.remove_cell(coordinate);
            }
            cell
        })
        .collect()
}

fn put_native_row_segment(
    worksheet: &mut Worksheet,
    row: usize,
    start_col: usize,
    cells: Vec<Option<umya_spreadsheet::Cell>>,
) {
    for (offset, cell) in cells.into_iter().enumerate() {
        let Some(mut cell) = cell else {
            continue;
        };
        let col = start_col + offset;
        cell.coordinate_mut()
            .set_col_num(col as u32 + 1)
            .set_row_num(row as u32 + 1);
        worksheet.set_cell(cell);
    }
}

fn marker_type(
    marker: &ImageMarker,
) -> umya_spreadsheet::structs::drawing::spreadsheet::MarkerType {
    let mut value = umya_spreadsheet::structs::drawing::spreadsheet::MarkerType::default();
    value
        .set_row(marker.row)
        .set_col(marker.col)
        .set_row_off(marker.row_offset_emu)
        .set_col_off(marker.col_offset_emu);
    value
}

fn apply_image_anchor(
    image: &mut umya_spreadsheet::Image,
    anchor: &ImageAnchor,
) -> Result<(), AppError> {
    let picture = image
        .one_cell_anchor()
        .and_then(|anchor| anchor.picture())
        .or_else(|| image.two_cell_anchor().and_then(|anchor| anchor.picture()))
        .cloned()
        .ok_or_else(|| {
            AppError::WorkbookPatchFailed("image picture data is missing".to_string())
        })?;

    match anchor {
        ImageAnchor::OneCell {
            from,
            width_emu,
            height_emu,
        } => {
            let mut value =
                umya_spreadsheet::structs::drawing::spreadsheet::OneCellAnchor::default();
            value.set_from_marker(marker_type(from));
            value.extent_mut().set_cx(*width_emu).set_cy(*height_emu);
            value.set_picture(picture);
            image.remove_two_cell_anchor().set_one_cell_anchor(value);
        }
        ImageAnchor::TwoCell { from, to } => {
            let mut value =
                umya_spreadsheet::structs::drawing::spreadsheet::TwoCellAnchor::default();
            value
                .set_from_marker(marker_type(from))
                .set_to_marker(marker_type(to))
                .set_picture(picture);
            image.remove_one_cell_anchor().set_two_cell_anchor(value);
        }
    }
    Ok(())
}

fn set_image_identity(image: &mut umya_spreadsheet::Image, image_id: &str) {
    if let Some(anchor) = image.one_cell_anchor_mut()
        && let Some(picture) = anchor.picture_mut()
    {
        picture
            .non_visual_picture_properties_mut()
            .non_visual_drawing_properties_mut()
            .set_name(format!("simple-table-image-{image_id}"));
    } else if let Some(anchor) = image.two_cell_anchor_mut()
        && let Some(picture) = anchor.picture_mut()
    {
        picture
            .non_visual_picture_properties_mut()
            .non_visual_drawing_properties_mut()
            .set_name(format!("simple-table-image-{image_id}"));
    }
}

pub fn workbook_capabilities(
    workbook: &Workbook,
    formula_structure_limitations: &[String],
) -> WorkbookCapabilities {
    let mut detected_features = Vec::new();
    let mut blocked_edit_reasons = Vec::new();
    let mut blocked_resize_reasons = Vec::new();
    let mut blocked_row_structure_reasons = Vec::new();
    let mut blocked_column_structure_reasons = Vec::new();
    let mut blocked_sheet_structure_reasons = Vec::new();
    let mut blocked_image_reasons = Vec::new();
    let mut global_sheet_reasons = SheetCapabilityReasons::default();

    if !workbook.defined_names().is_empty() {
        push_detected_feature(&mut detected_features, "workbook defined names");
        push_block_reason(&mut blocked_row_structure_reasons, "workbook defined names");
        push_block_reason(
            &mut blocked_column_structure_reasons,
            "workbook defined names",
        );
        push_block_reason(
            &mut blocked_sheet_structure_reasons,
            "workbook defined names",
        );
        global_sheet_reasons.block_row_structure("workbook defined names");
        global_sheet_reasons.block_column_structure("workbook defined names");
    }
    if workbook.workbook_protection().is_some() {
        push_detected_feature(&mut detected_features, "workbook protection");
        push_block_reason(&mut blocked_edit_reasons, "workbook protection");
        push_block_reason(&mut blocked_resize_reasons, "workbook protection");
        push_block_reason(&mut blocked_row_structure_reasons, "workbook protection");
        push_block_reason(&mut blocked_column_structure_reasons, "workbook protection");
        push_block_reason(&mut blocked_sheet_structure_reasons, "workbook protection");
        global_sheet_reasons.block_edit("workbook protection");
        global_sheet_reasons.block_resize("workbook protection");
        global_sheet_reasons.block_row_structure("workbook protection");
        global_sheet_reasons.block_column_structure("workbook protection");
        push_block_reason(&mut blocked_image_reasons, "workbook protection");
    }
    if workbook.has_threaded_comments() {
        push_detected_feature(&mut detected_features, "threaded comments");
        push_block_reason(&mut blocked_row_structure_reasons, "threaded comments");
        push_block_reason(&mut blocked_column_structure_reasons, "threaded comments");
        push_block_reason(&mut blocked_sheet_structure_reasons, "threaded comments");
        global_sheet_reasons.block_row_structure("threaded comments");
        global_sheet_reasons.block_column_structure("threaded comments");
    }
    for limitation in formula_structure_limitations {
        push_detected_feature(&mut detected_features, limitation);
        push_block_reason(&mut blocked_row_structure_reasons, limitation);
        push_block_reason(&mut blocked_column_structure_reasons, limitation);
        push_block_reason(&mut blocked_sheet_structure_reasons, limitation);
        global_sheet_reasons.block_row_structure(limitation);
        global_sheet_reasons.block_column_structure(limitation);
    }
    let mut sheets = Vec::with_capacity(workbook.sheet_count());
    for worksheet in workbook.sheet_collection() {
        let mut sheet_reasons = global_sheet_reasons.clone();
        if !worksheet.defined_names().is_empty() {
            push_detected_feature(&mut detected_features, "sheet defined names");
            push_block_reason(&mut blocked_row_structure_reasons, "sheet defined names");
            push_block_reason(&mut blocked_column_structure_reasons, "sheet defined names");
            push_block_reason(&mut blocked_sheet_structure_reasons, "sheet defined names");
            sheet_reasons.block_row_structure("sheet defined names");
            sheet_reasons.block_column_structure("sheet defined names");
        }
        if worksheet.has_table() {
            push_detected_feature(&mut detected_features, "tables");
            push_block_reason(&mut blocked_row_structure_reasons, "tables");
            push_block_reason(&mut blocked_column_structure_reasons, "tables");
            sheet_reasons.block_row_structure("tables");
            sheet_reasons.block_column_structure("tables");
        }
        if worksheet.has_pivot_table() {
            push_detected_feature(&mut detected_features, "pivot tables");
            push_block_reason(&mut blocked_row_structure_reasons, "pivot tables");
            push_block_reason(&mut blocked_column_structure_reasons, "pivot tables");
            push_block_reason(&mut blocked_sheet_structure_reasons, "pivot tables");
            sheet_reasons.block_row_structure("pivot tables");
            sheet_reasons.block_column_structure("pivot tables");
        }
        if !worksheet.chart_collection().is_empty() {
            push_detected_feature(&mut detected_features, "charts");
            push_block_reason(&mut blocked_row_structure_reasons, "charts");
            push_block_reason(&mut blocked_column_structure_reasons, "charts");
            sheet_reasons.block_row_structure("charts");
            sheet_reasons.block_column_structure("charts");
        }
        if !worksheet.image_collection().is_empty() {
            push_detected_feature(&mut detected_features, "images");
        }
        if worksheet.has_drawing_object()
            && worksheet.image_collection().is_empty()
            && worksheet.chart_collection().is_empty()
        {
            push_detected_feature(&mut detected_features, "unsupported drawings");
            push_block_reason(&mut blocked_row_structure_reasons, "unsupported drawings");
            push_block_reason(
                &mut blocked_column_structure_reasons,
                "unsupported drawings",
            );
            sheet_reasons.block_row_structure("unsupported drawings");
            sheet_reasons.block_column_structure("unsupported drawings");
        }
        if worksheet.data_validations().is_some() || worksheet.data_validations_2010().is_some() {
            push_detected_feature(&mut detected_features, "data validations");
            push_block_reason(&mut blocked_row_structure_reasons, "data validations");
            push_block_reason(&mut blocked_column_structure_reasons, "data validations");
            sheet_reasons.block_row_structure("data validations");
            sheet_reasons.block_column_structure("data validations");
        }
        if !worksheet.conditional_formatting_collection().is_empty() {
            push_detected_feature(&mut detected_features, "conditional formatting");
            push_block_reason(&mut blocked_row_structure_reasons, "conditional formatting");
            push_block_reason(
                &mut blocked_column_structure_reasons,
                "conditional formatting",
            );
            sheet_reasons.block_row_structure("conditional formatting");
            sheet_reasons.block_column_structure("conditional formatting");
        }
        if worksheet.auto_filter().is_some() {
            push_detected_feature(&mut detected_features, "auto filters");
            push_block_reason(&mut blocked_row_structure_reasons, "auto filters");
            sheet_reasons.block_row_structure("auto filters");
        }
        if worksheet.has_comments() {
            push_detected_feature(&mut detected_features, "comments");
            push_block_reason(&mut blocked_row_structure_reasons, "comments");
            push_block_reason(&mut blocked_column_structure_reasons, "comments");
            sheet_reasons.block_row_structure("comments");
            sheet_reasons.block_column_structure("comments");
        }
        if worksheet.has_threaded_comments() {
            push_detected_feature(&mut detected_features, "threaded comments");
            push_block_reason(&mut blocked_row_structure_reasons, "threaded comments");
            push_block_reason(&mut blocked_column_structure_reasons, "threaded comments");
            sheet_reasons.block_row_structure("threaded comments");
            sheet_reasons.block_column_structure("threaded comments");
        }
        if worksheet.sheet_protection().is_some() {
            push_detected_feature(&mut detected_features, "sheet protection");
            push_block_reason(&mut blocked_edit_reasons, "sheet protection");
            push_block_reason(&mut blocked_resize_reasons, "sheet protection");
            push_block_reason(&mut blocked_row_structure_reasons, "sheet protection");
            push_block_reason(&mut blocked_column_structure_reasons, "sheet protection");
            sheet_reasons.block_edit("sheet protection");
            sheet_reasons.block_resize("sheet protection");
            sheet_reasons.block_row_structure("sheet protection");
            sheet_reasons.block_column_structure("sheet protection");
            push_block_reason(&mut blocked_image_reasons, "sheet protection");
        }
        sheets.push(sheet_reasons.into_capabilities());
    }

    normalize_reasons(&mut detected_features);
    normalize_reasons(&mut blocked_edit_reasons);
    normalize_reasons(&mut blocked_resize_reasons);
    normalize_reasons(&mut blocked_row_structure_reasons);
    normalize_reasons(&mut blocked_column_structure_reasons);
    normalize_reasons(&mut blocked_sheet_structure_reasons);
    normalize_reasons(&mut blocked_image_reasons);

    let blocked_structure_reasons = merged_reasons([
        &blocked_row_structure_reasons,
        &blocked_column_structure_reasons,
        &blocked_sheet_structure_reasons,
    ]);

    if sheets.is_empty()
        && (!blocked_edit_reasons.is_empty()
            || !blocked_resize_reasons.is_empty()
            || !blocked_row_structure_reasons.is_empty()
            || !blocked_column_structure_reasons.is_empty())
    {
        sheets.push(
            SheetCapabilityReasons {
                blocked_edit_reasons,
                blocked_resize_reasons,
                blocked_row_structure_reasons,
                blocked_column_structure_reasons,
            }
            .into_capabilities(),
        );
    }

    WorkbookCapabilities {
        save: WorkbookSaveCapabilities {
            can_native_save: true,
            blocked_save_reasons: Vec::new(),
            detected_features,
        },
        structure: WorkbookStructureCapabilities {
            can_insert_delete_sheets: blocked_sheet_structure_reasons.is_empty(),
            blocked_structure_reasons,
            blocked_sheet_structure_reasons,
        },
        rich: WorkbookRichCapabilities {
            images: WorkbookImageCapabilities {
                can_insert: blocked_image_reasons.is_empty(),
                can_move_resize: blocked_image_reasons.is_empty(),
                can_delete: blocked_image_reasons.is_empty(),
                blocked_reasons: blocked_image_reasons,
            },
            ..WorkbookRichCapabilities::default()
        },
        sheets,
    }
}

#[derive(Clone, Default)]
struct SheetCapabilityReasons {
    blocked_edit_reasons: Vec<String>,
    blocked_resize_reasons: Vec<String>,
    blocked_row_structure_reasons: Vec<String>,
    blocked_column_structure_reasons: Vec<String>,
}

impl SheetCapabilityReasons {
    fn block_edit(&mut self, reason: &str) {
        push_block_reason(&mut self.blocked_edit_reasons, reason);
    }

    fn block_resize(&mut self, reason: &str) {
        push_block_reason(&mut self.blocked_resize_reasons, reason);
    }

    fn block_row_structure(&mut self, reason: &str) {
        push_block_reason(&mut self.blocked_row_structure_reasons, reason);
    }

    fn block_column_structure(&mut self, reason: &str) {
        push_block_reason(&mut self.blocked_column_structure_reasons, reason);
    }

    fn into_capabilities(mut self) -> SheetCapabilities {
        normalize_reasons(&mut self.blocked_edit_reasons);
        normalize_reasons(&mut self.blocked_resize_reasons);
        normalize_reasons(&mut self.blocked_row_structure_reasons);
        normalize_reasons(&mut self.blocked_column_structure_reasons);

        SheetCapabilities {
            can_edit_cells: self.blocked_edit_reasons.is_empty(),
            can_resize_rows_columns: self.blocked_resize_reasons.is_empty(),
            can_insert_delete_rows: self.blocked_row_structure_reasons.is_empty(),
            can_insert_delete_columns: self.blocked_column_structure_reasons.is_empty(),
            blocked_edit_reasons: self.blocked_edit_reasons,
            blocked_resize_reasons: self.blocked_resize_reasons,
            blocked_row_structure_reasons: self.blocked_row_structure_reasons,
            blocked_column_structure_reasons: self.blocked_column_structure_reasons,
        }
    }
}

pub fn unsupported_operation_features(
    workbook: &Workbook,
    operation: &AppliedOperation,
    formula_structure_limitations: &[String],
) -> Vec<String> {
    let mut reasons = Vec::new();

    if workbook.workbook_protection().is_some() {
        push_block_reason(&mut reasons, "workbook protection");
    }

    if operation.impact().is_structure_change() {
        if !workbook.defined_names().is_empty() {
            push_block_reason(&mut reasons, "workbook defined names");
        }
        if workbook.has_threaded_comments() {
            push_block_reason(&mut reasons, "threaded comments");
        }
        for limitation in formula_structure_limitations {
            push_block_reason(&mut reasons, limitation);
        }
    }

    for sheet_index in operation_blocker_sheet_indexes(workbook, operation) {
        let Ok(worksheet) = workbook.sheet(sheet_index) else {
            continue;
        };
        push_worksheet_operation_blockers(&mut reasons, worksheet, operation);
    }

    normalize_reasons(&mut reasons);
    reasons
}

fn operation_blocker_sheet_indexes(
    workbook: &Workbook,
    operation: &AppliedOperation,
) -> Vec<usize> {
    match operation {
        AppliedOperation::SetCell { sheet_index, .. }
        | AppliedOperation::SetColumnWidth { sheet_index, .. }
        | AppliedOperation::SetRowHeight { sheet_index, .. }
        | AppliedOperation::AddRow { sheet_index, .. }
        | AppliedOperation::DeleteRow { sheet_index, .. }
        | AppliedOperation::AddColumn { sheet_index, .. }
        | AppliedOperation::DeleteColumn { sheet_index, .. } => vec![*sheet_index],
        AppliedOperation::SortRows(sort) => vec![sort.sheet_index],
        AppliedOperation::InsertImage { sheet_index, .. }
        | AppliedOperation::UpdateImage { sheet_index, .. }
        | AppliedOperation::DeleteImage { sheet_index, .. } => vec![*sheet_index],
        AppliedOperation::SetCells { changes } => {
            let mut indexes: Vec<usize> = changes.iter().map(|change| change.sheet_index).collect();
            indexes.sort_unstable();
            indexes.dedup();
            indexes
        }
        AppliedOperation::AddSheet { .. } | AppliedOperation::DeleteSheet { .. } => {
            (0..workbook.sheet_count()).collect()
        }
    }
}

fn push_worksheet_operation_blockers(
    reasons: &mut Vec<String>,
    worksheet: &Worksheet,
    operation: &AppliedOperation,
) {
    let reorders_cells = operation.impact().is_row_structure_change()
        || operation.impact().is_column_structure_change()
        || matches!(operation, AppliedOperation::SortRows(_));
    let reorders_rows = operation.impact().is_row_structure_change()
        || matches!(operation, AppliedOperation::SortRows(_));
    if worksheet.sheet_protection().is_some()
        && (operation.impact().is_cell_edit()
            || operation.impact().is_layout_change()
            || operation.impact().is_row_structure_change()
            || operation.impact().is_column_structure_change()
            || operation.impact().is_image_change())
    {
        push_block_reason(reasons, "sheet protection");
    }

    if operation.impact().is_structure_change() && !worksheet.defined_names().is_empty() {
        push_block_reason(reasons, "sheet defined names");
    }
    if reorders_cells && worksheet.has_table() {
        push_block_reason(reasons, "tables");
    }
    if operation.impact().is_structure_change() && worksheet.has_pivot_table() {
        push_block_reason(reasons, "pivot tables");
    }
    if reorders_cells && !worksheet.chart_collection().is_empty() {
        push_block_reason(reasons, "charts");
    }
    if reorders_cells
        && worksheet.has_drawing_object()
        && worksheet.image_collection().is_empty()
        && worksheet.chart_collection().is_empty()
    {
        push_block_reason(reasons, "unsupported drawings");
    }
    if reorders_cells
        && (worksheet.data_validations().is_some() || worksheet.data_validations_2010().is_some())
    {
        push_block_reason(reasons, "data validations");
    }
    if reorders_cells && !worksheet.conditional_formatting_collection().is_empty() {
        push_block_reason(reasons, "conditional formatting");
    }
    if reorders_rows && worksheet.auto_filter().is_some() {
        push_block_reason(reasons, "auto filters");
    }
    if reorders_cells && worksheet.has_comments() {
        push_block_reason(reasons, "comments");
    }
    if reorders_cells && worksheet.has_threaded_comments() {
        push_block_reason(reasons, "threaded comments");
    }
}

fn push_detected_feature(detected_features: &mut Vec<String>, feature: &str) {
    detected_features.push(feature.to_string());
}

fn push_block_reason(reasons: &mut Vec<String>, reason: &str) {
    reasons.push(reason.to_string());
}

fn normalize_reasons(reasons: &mut Vec<String>) {
    reasons.sort_unstable();
    reasons.dedup();
}

fn merged_reasons<const N: usize>(groups: [&Vec<String>; N]) -> Vec<String> {
    let mut reasons = Vec::new();
    for group in groups {
        reasons.extend(group.iter().cloned());
    }
    normalize_reasons(&mut reasons);
    reasons
}

fn rewrite_formulas_after_native_structure_shift(
    workbook: &mut Workbook,
    ast_service: &mut FormulaAstService,
    plan: FormulaRewritePlan<'_>,
) -> usize {
    let mut skipped = 0;
    for worksheet in workbook.sheet_collection_mut() {
        let current_sheet_name = worksheet.name().to_string();
        if should_rewrite_formula_sheet(&current_sheet_name, plan.target_sheet_name, plan.scope) {
            skipped += adjust_worksheet_formulas(
                ast_service,
                worksheet,
                plan.target_sheet_name,
                &current_sheet_name,
                plan.shift,
            );
        } else if current_sheet_name == plan.target_sheet_name {
            skipped += adjust_worksheet_explicit_sheet_name_case_mismatches(
                ast_service,
                worksheet,
                plan.target_sheet_name,
                &current_sheet_name,
                plan.shift,
            );
        }
    }
    skipped
}

fn should_rewrite_formula_sheet(
    current_sheet_name: &str,
    target_sheet_name: &str,
    scope: FormulaRewriteScope,
) -> bool {
    match scope {
        FormulaRewriteScope::CrossSheetOnly => current_sheet_name != target_sheet_name,
    }
}

pub fn patch_formula_changes(
    workbook: &mut Workbook,
    file_data: &mut DocumentData,
    cell_changes: &[DocumentCellChange],
    backing: &dyn WorkbookBackingPort,
) -> Result<(), AppError> {
    patch_cell_changes(workbook, file_data, cell_changes, backing)
}

pub fn patch_layout_dimensions(
    workbook: &mut Workbook,
    sheet_index: usize,
    column_widths: &HashMap<usize, Option<u32>>,
    row_heights: &HashMap<usize, Option<u32>>,
    backing: &dyn WorkbookBackingPort,
) -> Result<(), AppError> {
    if let Some(worksheet) = sheet_mut(workbook, sheet_index)? {
        for (col_index, width) in column_widths {
            patch_column_width(worksheet, *col_index, *width, backing);
        }
        for (row_index, height) in row_heights {
            patch_row_height(worksheet, *row_index, *height, backing);
        }
    }
    Ok(())
}

pub fn patch_cell_shapes(
    workbook: &mut Workbook,
    sheet_shapes: &[WorkbookSheetShape],
) -> Result<(), AppError> {
    for shape in sheet_shapes {
        let Some(worksheet) = sheet_mut(workbook, shape.sheet_index)? else {
            continue;
        };
        let target_rows = shape.row_lengths.len() as u32;
        let existing_cells: Vec<(u32, u32)> = worksheet
            .cells()
            .iter()
            .map(|cell| (cell.coordinate().col_num(), cell.coordinate().row_num()))
            .collect();

        let protected_cells: std::collections::HashSet<(usize, usize)> =
            shape.protected_cells.iter().copied().collect();
        for (col, row) in existing_cells {
            let row_index = row.saturating_sub(1) as usize;
            let col_index = col.saturating_sub(1) as usize;
            if protected_cells.contains(&(row_index, col_index)) {
                continue;
            }
            let target_width = row
                .checked_sub(1)
                .and_then(|row_index| shape.row_lengths.get(row_index as usize).copied())
                .unwrap_or(0) as u32;
            if row > target_rows || col > target_width {
                worksheet.remove_cell((col, row));
            }
        }
    }
    Ok(())
}

fn patch_cell_changes(
    workbook: &mut Workbook,
    file_data: &mut DocumentData,
    cell_changes: &[DocumentCellChange],
    backing: &dyn WorkbookBackingPort,
) -> Result<(), AppError> {
    for change in cell_changes {
        patch_cell(
            workbook,
            file_data,
            change.sheet_index,
            change.row,
            change.col,
            backing,
        )?;
    }
    Ok(())
}

fn adjust_worksheet_formulas(
    ast_service: &mut FormulaAstService,
    worksheet: &mut Worksheet,
    target_sheet_name: &str,
    current_sheet_name: &str,
    shift: StructureShift,
) -> usize {
    let mut skipped = 0;
    for cell in worksheet.cells_mut() {
        if !cell.is_formula() {
            continue;
        }
        let rewrite = adjust_formula_references(
            ast_service,
            cell.formula(),
            target_sheet_name,
            current_sheet_name,
            shift,
        );
        if rewrite.skipped {
            skipped += 1;
            continue;
        }
        if rewrite.formula != cell.formula() {
            cell.set_formula(rewrite.formula);
        }
    }
    skipped
}

fn adjust_worksheet_explicit_sheet_name_case_mismatches(
    ast_service: &mut FormulaAstService,
    worksheet: &mut Worksheet,
    target_sheet_name: &str,
    current_sheet_name: &str,
    shift: StructureShift,
) -> usize {
    let mut skipped = 0;
    for cell in worksheet.cells_mut() {
        if !cell.is_formula() {
            continue;
        }
        let rewrite = adjust_explicit_sheet_name_case_mismatched_references(
            ast_service,
            cell.formula(),
            target_sheet_name,
            current_sheet_name,
            shift,
        );
        if rewrite.skipped {
            skipped += 1;
            continue;
        }
        if rewrite.formula != cell.formula() {
            cell.set_formula(rewrite.formula);
        }
    }
    skipped
}

fn invalidate_sheet_references_before_delete(
    workbook: &mut Workbook,
    ast_service: &mut FormulaAstService,
    sheet_index: usize,
) -> Result<usize, AppError> {
    let deleted_sheet_name = sheet_name(workbook, sheet_index)?;
    let mut skipped = 0;
    for (current_index, worksheet) in workbook.sheet_collection_mut().iter_mut().enumerate() {
        if current_index == sheet_index {
            continue;
        }
        let current_sheet_name = worksheet.name().to_string();
        for cell in worksheet.cells_mut() {
            if !cell.is_formula() {
                continue;
            }
            let rewrite = invalidate_deleted_sheet_references(
                ast_service,
                cell.formula(),
                &deleted_sheet_name,
                &current_sheet_name,
            );
            if rewrite.skipped {
                skipped += 1;
                continue;
            }
            if rewrite.formula != cell.formula() {
                cell.set_formula(rewrite.formula);
            }
        }
    }
    Ok(skipped)
}

fn patch_cell(
    workbook: &mut Workbook,
    file_data: &DocumentData,
    sheet_index: usize,
    row: usize,
    col: usize,
    backing: &dyn WorkbookBackingPort,
) -> Result<(), AppError> {
    let Some(cell_value) = file_data
        .sheets
        .get(sheet_index)
        .and_then(|sheet| sheet.rows.get(row))
        .and_then(|row_data| row_data.get(col))
    else {
        return Ok(());
    };

    if let Some(worksheet) = sheet_mut(workbook, sheet_index)? {
        backing.write_cell(worksheet, row as u32 + 1, col as u32 + 1, cell_value);
    }
    Ok(())
}

fn patch_column_width(
    worksheet: &mut Worksheet,
    col_index: usize,
    width: Option<u32>,
    backing: &dyn WorkbookBackingPort,
) {
    let col_num = col_index as u32 + 1;
    match width {
        Some(width) => {
            worksheet
                .column_dimension_by_number_mut(col_num)
                .set_width(backing.column_width_from_pixels(width));
        }
        None => {
            worksheet
                .column_dimensions_mut()
                .retain(|column| column.col_num() != col_num);
        }
    }
}

fn patch_row_height(
    worksheet: &mut Worksheet,
    row_index: usize,
    height: Option<u32>,
    backing: &dyn WorkbookBackingPort,
) {
    let row_num = row_index as u32 + 1;
    match height {
        Some(height) => {
            worksheet
                .row_dimension_mut(row_num)
                .set_height(backing.row_height_from_pixels(height));
        }
        None => {
            worksheet.row_dimensions_to_hashmap_mut().remove(&row_num);
        }
    }
}

fn insert_sheet(
    workbook: &mut Workbook,
    sheet_index: usize,
    sheet_data: &DocumentSheet,
    backing: &dyn WorkbookBackingPort,
) -> Result<(), AppError> {
    let sheet_name = if sheet_data.name.is_empty() {
        format!("Sheet{}", sheet_index + 1)
    } else {
        sheet_data.name.clone()
    };

    workbook
        .new_sheet(sheet_name)
        .map_err(|e| AppError::WriteError(e.to_string()))?;

    let last_index = workbook.sheet_count().saturating_sub(1);
    if sheet_index < last_index {
        let sheets = workbook.sheet_collection_mut();
        for index in (sheet_index..last_index).rev() {
            sheets.swap(index, index + 1);
        }
    }

    let worksheet = workbook
        .sheet_mut(sheet_index)
        .map_err(|e| AppError::WriteError(e.to_string()))?;
    backing.sync_sheet(worksheet, sheet_data)
}

fn remove_sheet(workbook: &mut Workbook, sheet_index: usize) -> Result<(), AppError> {
    if workbook.sheet_count() <= 1 {
        return Ok(());
    }
    if sheet_index < workbook.sheet_count() {
        workbook
            .remove_sheet(sheet_index)
            .map_err(|e| AppError::WriteError(e.to_string()))?;
    }
    Ok(())
}

fn sheet_mut(
    workbook: &mut Workbook,
    sheet_index: usize,
) -> Result<Option<&mut Worksheet>, AppError> {
    if sheet_index >= workbook.sheet_count() {
        return Ok(None);
    }
    workbook
        .sheet_mut(sheet_index)
        .map(Some)
        .map_err(|e| AppError::WriteError(e.to_string()))
}

fn sheet_name(workbook: &Workbook, sheet_index: usize) -> Result<String, AppError> {
    workbook
        .sheet(sheet_index)
        .map(|sheet| sheet.name().to_string())
        .map_err(|e| AppError::WriteError(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unparseable_formulas_block_structure_editing() {
        let mut workbook = umya_spreadsheet::new_file();
        workbook
            .sheet_mut(0)
            .expect("sheet")
            .cell_mut("A1")
            .set_formula("SUM(");

        let capabilities = workbook_capabilities(&workbook, &["unparseable formulas".to_string()]);

        assert!(!capabilities.sheets[0].can_insert_delete_rows);
        assert!(!capabilities.sheets[0].can_insert_delete_columns);
        assert!(!capabilities.structure.can_insert_delete_sheets);
        assert!(
            capabilities
                .structure
                .blocked_structure_reasons
                .iter()
                .any(|reason| reason == "unparseable formulas")
        );
    }
}
