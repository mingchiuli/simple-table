use std::collections::HashMap;

use crate::error::AppError;
use crate::formula::reference_rewrite::{
    StructureShift, adjust_formula_references, invalidate_deleted_sheet_references,
};
use crate::io::codec::reader::read_worksheet;
use crate::io::codec::writer::{
    coordinate, px_to_excel_column_width, px_to_points, sync_sheet_from_sheet_data, write_cell,
};
use crate::ops::AppliedOperation;
use crate::types::{FileData, SheetCellChange, SheetData, WorkbookCapabilities};
use umya_spreadsheet::{Workbook, Worksheet};

#[derive(Clone, Copy, Debug, Default)]
pub struct StructurePatchDiagnostics {
    pub skipped_formula_reference_rewrites: usize,
}

pub fn patch_after_operation(
    workbook: &mut Workbook,
    file_data: &mut FileData,
    operation: &AppliedOperation,
    cell_changes: &[SheetCellChange],
) -> Result<(), AppError> {
    match operation {
        AppliedOperation::SetCell {
            sheet_index,
            row,
            col,
            ..
        } => {
            patch_cell(workbook, file_data, *sheet_index, *row, *col)?;
            patch_cell_changes(workbook, file_data, cell_changes)?;
        }
        AppliedOperation::SetCells { changes } => {
            for change in changes {
                patch_cell(
                    workbook,
                    file_data,
                    change.sheet_index,
                    change.row,
                    change.col,
                )?;
            }
            patch_cell_changes(workbook, file_data, cell_changes)?;
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
                patch_column_width(worksheet, *col_index, *new_width);
            }
        }
        AppliedOperation::SetRowHeight {
            sheet_index,
            row_index,
            new_height,
            ..
        } => {
            if let Some(worksheet) = sheet_mut(workbook, *sheet_index)? {
                patch_row_height(worksheet, *row_index, *new_height);
            }
        }
    }

    Ok(())
}

pub fn apply_structure_operation(
    workbook: &mut Workbook,
    operation: &AppliedOperation,
) -> Result<StructurePatchDiagnostics, AppError> {
    let unsupported = unsupported_structure_features(workbook, operation);
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
            diagnostics.skipped_formula_reference_rewrites += adjust_other_sheet_formulas(
                workbook,
                &sheet_name,
                StructureShift::InsertRows {
                    row_index: *row_index,
                    count: 1,
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
            diagnostics.skipped_formula_reference_rewrites += adjust_other_sheet_formulas(
                workbook,
                &sheet_name,
                StructureShift::DeleteRows {
                    row_index: *row_index,
                    count: 1,
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
            diagnostics.skipped_formula_reference_rewrites += adjust_other_sheet_formulas(
                workbook,
                &sheet_name,
                StructureShift::InsertColumns {
                    col_index: *col_index,
                    count: 1,
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
            diagnostics.skipped_formula_reference_rewrites += adjust_other_sheet_formulas(
                workbook,
                &sheet_name,
                StructureShift::DeleteColumns {
                    col_index: *col_index,
                    count: 1,
                },
            );
        }
        AppliedOperation::AddSheet {
            sheet_index,
            sheet_data,
        } => {
            insert_sheet(workbook, *sheet_index, sheet_data)?;
        }
        AppliedOperation::DeleteSheet { sheet_index } => {
            diagnostics.skipped_formula_reference_rewrites +=
                invalidate_sheet_references_before_delete(workbook, *sheet_index)?;
            remove_sheet(workbook, *sheet_index)?;
        }
        AppliedOperation::SetCell { .. }
        | AppliedOperation::SetCells { .. }
        | AppliedOperation::SetColumnWidth { .. }
        | AppliedOperation::SetRowHeight { .. } => {}
    }

    Ok(diagnostics)
}

pub fn workbook_capabilities(workbook: &Workbook) -> WorkbookCapabilities {
    let mut detected_features = Vec::new();
    let mut blocked_edit_reasons = Vec::new();
    let mut blocked_resize_reasons = Vec::new();
    let mut blocked_row_structure_reasons = Vec::new();
    let mut blocked_column_structure_reasons = Vec::new();
    let mut blocked_sheet_structure_reasons = Vec::new();

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
    }
    if workbook.workbook_protection().is_some() {
        push_detected_feature(&mut detected_features, "workbook protection");
        push_block_reason(&mut blocked_edit_reasons, "workbook protection");
        push_block_reason(&mut blocked_resize_reasons, "workbook protection");
        push_block_reason(&mut blocked_row_structure_reasons, "workbook protection");
        push_block_reason(&mut blocked_column_structure_reasons, "workbook protection");
        push_block_reason(&mut blocked_sheet_structure_reasons, "workbook protection");
    }
    if workbook.has_threaded_comments() {
        push_detected_feature(&mut detected_features, "threaded comments");
        push_block_reason(&mut blocked_row_structure_reasons, "threaded comments");
        push_block_reason(&mut blocked_column_structure_reasons, "threaded comments");
        push_block_reason(&mut blocked_sheet_structure_reasons, "threaded comments");
    }
    for worksheet in workbook.sheet_collection() {
        if !worksheet.defined_names().is_empty() {
            push_detected_feature(&mut detected_features, "sheet defined names");
            push_block_reason(&mut blocked_row_structure_reasons, "sheet defined names");
            push_block_reason(&mut blocked_column_structure_reasons, "sheet defined names");
            push_block_reason(&mut blocked_sheet_structure_reasons, "sheet defined names");
        }
        if worksheet.has_table() {
            push_detected_feature(&mut detected_features, "tables");
            push_block_reason(&mut blocked_row_structure_reasons, "tables");
            push_block_reason(&mut blocked_column_structure_reasons, "tables");
        }
        if worksheet.has_pivot_table() {
            push_detected_feature(&mut detected_features, "pivot tables");
            push_block_reason(&mut blocked_row_structure_reasons, "pivot tables");
            push_block_reason(&mut blocked_column_structure_reasons, "pivot tables");
            push_block_reason(&mut blocked_sheet_structure_reasons, "pivot tables");
        }
        if !worksheet.chart_collection().is_empty() {
            push_detected_feature(&mut detected_features, "charts");
            push_block_reason(&mut blocked_row_structure_reasons, "charts");
            push_block_reason(&mut blocked_column_structure_reasons, "charts");
        }
        if !worksheet.image_collection().is_empty() || worksheet.has_drawing_object() {
            push_detected_feature(&mut detected_features, "drawings/images");
            push_block_reason(&mut blocked_row_structure_reasons, "drawings/images");
            push_block_reason(&mut blocked_column_structure_reasons, "drawings/images");
        }
        if worksheet.data_validations().is_some() || worksheet.data_validations_2010().is_some() {
            push_detected_feature(&mut detected_features, "data validations");
            push_block_reason(&mut blocked_row_structure_reasons, "data validations");
            push_block_reason(&mut blocked_column_structure_reasons, "data validations");
        }
        if !worksheet.conditional_formatting_collection().is_empty() {
            push_detected_feature(&mut detected_features, "conditional formatting");
            push_block_reason(&mut blocked_row_structure_reasons, "conditional formatting");
            push_block_reason(
                &mut blocked_column_structure_reasons,
                "conditional formatting",
            );
        }
        if worksheet.auto_filter().is_some() {
            push_detected_feature(&mut detected_features, "auto filters");
            push_block_reason(&mut blocked_row_structure_reasons, "auto filters");
        }
        if worksheet.has_comments() {
            push_detected_feature(&mut detected_features, "comments");
            push_block_reason(&mut blocked_row_structure_reasons, "comments");
            push_block_reason(&mut blocked_column_structure_reasons, "comments");
        }
        if worksheet.has_threaded_comments() {
            push_detected_feature(&mut detected_features, "threaded comments");
            push_block_reason(&mut blocked_row_structure_reasons, "threaded comments");
            push_block_reason(&mut blocked_column_structure_reasons, "threaded comments");
        }
        if worksheet.sheet_protection().is_some() {
            push_detected_feature(&mut detected_features, "sheet protection");
            push_block_reason(&mut blocked_edit_reasons, "sheet protection");
            push_block_reason(&mut blocked_resize_reasons, "sheet protection");
            push_block_reason(&mut blocked_row_structure_reasons, "sheet protection");
            push_block_reason(&mut blocked_column_structure_reasons, "sheet protection");
        }
    }

    normalize_reasons(&mut detected_features);
    normalize_reasons(&mut blocked_edit_reasons);
    normalize_reasons(&mut blocked_resize_reasons);
    normalize_reasons(&mut blocked_row_structure_reasons);
    normalize_reasons(&mut blocked_column_structure_reasons);
    normalize_reasons(&mut blocked_sheet_structure_reasons);

    let blocked_structure_reasons = merged_reasons([
        &blocked_row_structure_reasons,
        &blocked_column_structure_reasons,
        &blocked_sheet_structure_reasons,
    ]);

    WorkbookCapabilities {
        can_edit_cells: blocked_edit_reasons.is_empty(),
        can_resize_rows_columns: blocked_resize_reasons.is_empty(),
        can_insert_delete_rows: blocked_row_structure_reasons.is_empty(),
        can_insert_delete_columns: blocked_column_structure_reasons.is_empty(),
        can_insert_delete_sheets: blocked_sheet_structure_reasons.is_empty(),
        can_native_save: true,
        blocked_structure_reasons,
        blocked_edit_reasons,
        blocked_resize_reasons,
        blocked_row_structure_reasons,
        blocked_column_structure_reasons,
        blocked_sheet_structure_reasons,
        detected_features,
    }
}

pub fn unsupported_structure_features(
    workbook: &Workbook,
    operation: &AppliedOperation,
) -> Vec<String> {
    let capabilities = workbook_capabilities(workbook);
    if operation.is_row_structure_change() {
        return capabilities.blocked_row_structure_reasons;
    }
    if operation.is_column_structure_change() {
        return capabilities.blocked_column_structure_reasons;
    }
    if operation.is_sheet_structure_change() {
        return capabilities.blocked_sheet_structure_reasons;
    }
    Vec::new()
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

fn adjust_other_sheet_formulas(
    workbook: &mut Workbook,
    target_sheet_name: &str,
    shift: StructureShift,
) -> usize {
    let mut skipped = 0;
    for worksheet in workbook.sheet_collection_mut() {
        let current_sheet_name = worksheet.name().to_string();
        if current_sheet_name == target_sheet_name {
            continue;
        }
        skipped +=
            adjust_worksheet_formulas(worksheet, target_sheet_name, &current_sheet_name, shift);
    }
    skipped
}

pub fn patch_formula_changes(
    workbook: &mut Workbook,
    file_data: &mut FileData,
    cell_changes: &[SheetCellChange],
) -> Result<(), AppError> {
    patch_cell_changes(workbook, file_data, cell_changes)
}

pub fn patch_layout_dimensions(
    workbook: &mut Workbook,
    sheet_index: usize,
    column_widths: &HashMap<usize, Option<u32>>,
    row_heights: &HashMap<usize, Option<u32>>,
) -> Result<(), AppError> {
    if let Some(worksheet) = sheet_mut(workbook, sheet_index)? {
        for (col_index, width) in column_widths {
            patch_column_width(worksheet, *col_index, *width);
        }
        for (row_index, height) in row_heights {
            patch_row_height(worksheet, *row_index, *height);
        }
    }
    Ok(())
}

pub fn patch_cell_shapes(
    workbook: &mut Workbook,
    sheet_shapes: &[(usize, Vec<usize>)],
) -> Result<(), AppError> {
    for (sheet_index, row_lengths) in sheet_shapes {
        let Some(worksheet) = sheet_mut(workbook, *sheet_index)? else {
            continue;
        };
        let (highest_col, highest_row) = worksheet.highest_column_and_row();
        let target_rows = row_lengths.len() as u32;

        for row in 1..=highest_row {
            let target_width = row
                .checked_sub(1)
                .and_then(|row_index| row_lengths.get(row_index as usize).copied())
                .unwrap_or(0) as u32;
            for col in 1..=highest_col {
                if row > target_rows || col > target_width {
                    worksheet.remove_cell((col, row));
                }
            }
        }
    }
    Ok(())
}

pub fn sync_all_merge_ranges_from_projection(
    workbook: &mut Workbook,
    file_data: &FileData,
) -> Result<(), AppError> {
    for sheet_index in 0..file_data.sheets.len() {
        let Some(worksheet) = sheet_mut(workbook, sheet_index)? else {
            continue;
        };
        worksheet.merge_cells_mut().clear();
        let Some(sheet) = file_data.sheets.get(sheet_index) else {
            continue;
        };
        for merge in &sheet.merges {
            let range = format!(
                "{}:{}",
                coordinate(merge.start_col as u32 + 1, merge.start_row + 1),
                coordinate(merge.end_col as u32 + 1, merge.end_row + 1)
            );
            worksheet.add_merge_cells(range);
        }
    }
    Ok(())
}

fn patch_cell_changes(
    workbook: &mut Workbook,
    file_data: &mut FileData,
    cell_changes: &[SheetCellChange],
) -> Result<(), AppError> {
    for change in cell_changes {
        patch_cell(
            workbook,
            file_data,
            change.sheet_index,
            change.row,
            change.col,
        )?;
    }
    Ok(())
}

fn adjust_worksheet_formulas(
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
        let rewrite =
            adjust_formula_references(cell.formula(), target_sheet_name, current_sheet_name, shift);
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
    file_data: &FileData,
    sheet_index: usize,
    row: usize,
    col: usize,
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
        write_cell(worksheet, row as u32 + 1, col as u32 + 1, cell_value);
    }
    Ok(())
}

pub fn refresh_projection_from_workbook(workbook: &Workbook, file_data: &mut FileData) {
    file_data.sheets = workbook
        .sheet_collection()
        .iter()
        .map(read_worksheet)
        .collect();
}

fn patch_column_width(worksheet: &mut Worksheet, col_index: usize, width: Option<u32>) {
    let col_num = col_index as u32 + 1;
    match width {
        Some(width) => {
            worksheet
                .column_dimension_by_number_mut(col_num)
                .set_width(px_to_excel_column_width(width));
        }
        None => {
            worksheet
                .column_dimensions_mut()
                .retain(|column| column.col_num() != col_num);
        }
    }
}

fn patch_row_height(worksheet: &mut Worksheet, row_index: usize, height: Option<u32>) {
    let row_num = row_index as u32 + 1;
    match height {
        Some(height) => {
            worksheet
                .row_dimension_mut(row_num)
                .set_height(px_to_points(height));
        }
        None => {
            worksheet.row_dimensions_to_hashmap_mut().remove(&row_num);
        }
    }
}

fn insert_sheet(
    workbook: &mut Workbook,
    sheet_index: usize,
    sheet_data: &SheetData,
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
    sync_sheet_from_sheet_data(worksheet, sheet_data)
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
