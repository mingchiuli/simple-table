use std::collections::HashMap;

use crate::domain::AppliedOperation;
use crate::error::AppError;
use crate::formula::ast::FormulaAstService;
use crate::formula::reference_rewrite::{
    StructureShift, adjust_explicit_sheet_name_case_mismatched_references,
    adjust_formula_references, invalidate_deleted_sheet_references,
};
use crate::io::codec::writer::{sync_sheet_from_sheet_data, write_cell};
use crate::io::document_body::BodySheetShape;
use crate::io::layout_units::{px_to_excel_column_width, px_to_points};
use crate::types::{
    FileData, SheetCapabilities, SheetCellChange, SheetData, WorkbookCapabilities,
    WorkbookRichCapabilities, WorkbookSaveCapabilities, WorkbookStructureCapabilities,
};
use umya_spreadsheet::{Workbook, Worksheet};

#[derive(Clone, Copy, Debug, Default)]
pub struct StructurePatchDiagnostics {
    pub skipped_formula_reference_rewrites: usize,
}

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
    ast_service: &mut FormulaAstService,
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
            sheet_data,
        } => {
            insert_sheet(workbook, *sheet_index, sheet_data)?;
        }
        AppliedOperation::DeleteSheet { sheet_index } => {
            diagnostics.skipped_formula_reference_rewrites +=
                invalidate_sheet_references_before_delete(workbook, ast_service, *sheet_index)?;
            remove_sheet(workbook, *sheet_index)?;
        }
        AppliedOperation::SetCell { .. }
        | AppliedOperation::SetCells { .. }
        | AppliedOperation::SetColumnWidth { .. }
        | AppliedOperation::SetRowHeight { .. } => {}
    }

    Ok(diagnostics)
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
        if !worksheet.image_collection().is_empty() || worksheet.has_drawing_object() {
            push_detected_feature(&mut detected_features, "drawings/images");
            push_block_reason(&mut blocked_row_structure_reasons, "drawings/images");
            push_block_reason(&mut blocked_column_structure_reasons, "drawings/images");
            sheet_reasons.block_row_structure("drawings/images");
            sheet_reasons.block_column_structure("drawings/images");
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
        }
        sheets.push(sheet_reasons.into_capabilities());
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
        rich: WorkbookRichCapabilities::default(),
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
    if worksheet.sheet_protection().is_some()
        && (operation.impact().is_cell_edit()
            || operation.impact().is_layout_change()
            || operation.impact().is_row_structure_change()
            || operation.impact().is_column_structure_change())
    {
        push_block_reason(reasons, "sheet protection");
    }

    if operation.impact().is_structure_change() && !worksheet.defined_names().is_empty() {
        push_block_reason(reasons, "sheet defined names");
    }
    if (operation.impact().is_row_structure_change()
        || operation.impact().is_column_structure_change())
        && worksheet.has_table()
    {
        push_block_reason(reasons, "tables");
    }
    if operation.impact().is_structure_change() && worksheet.has_pivot_table() {
        push_block_reason(reasons, "pivot tables");
    }
    if (operation.impact().is_row_structure_change()
        || operation.impact().is_column_structure_change())
        && !worksheet.chart_collection().is_empty()
    {
        push_block_reason(reasons, "charts");
    }
    if (operation.impact().is_row_structure_change()
        || operation.impact().is_column_structure_change())
        && (!worksheet.image_collection().is_empty() || worksheet.has_drawing_object())
    {
        push_block_reason(reasons, "drawings/images");
    }
    if (operation.impact().is_row_structure_change()
        || operation.impact().is_column_structure_change())
        && (worksheet.data_validations().is_some() || worksheet.data_validations_2010().is_some())
    {
        push_block_reason(reasons, "data validations");
    }
    if (operation.impact().is_row_structure_change()
        || operation.impact().is_column_structure_change())
        && !worksheet.conditional_formatting_collection().is_empty()
    {
        push_block_reason(reasons, "conditional formatting");
    }
    if operation.impact().is_row_structure_change() && worksheet.auto_filter().is_some() {
        push_block_reason(reasons, "auto filters");
    }
    if (operation.impact().is_row_structure_change()
        || operation.impact().is_column_structure_change())
        && worksheet.has_comments()
    {
        push_block_reason(reasons, "comments");
    }
    if (operation.impact().is_row_structure_change()
        || operation.impact().is_column_structure_change())
        && worksheet.has_threaded_comments()
    {
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
    sheet_shapes: &[BodySheetShape],
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
