use crate::document_data::{DocumentData, DocumentSheet, RichMetadata};
use std::collections::HashMap;

use crate::domain::cell_key::parse_cell_key;
use crate::domain::{
    AppliedOperation, CellValue, EditorCommand, ResolvedCellEdit, parse_cell_text,
};
use crate::error::AppError;
use crate::resource_limits::{
    ResourceLedger, validate_added_sheet, validate_column_width, validate_row_height,
};

impl EditorCommand {
    #[cfg(test)]
    pub fn resolve(self, file_data: &DocumentData) -> Result<AppliedOperation, AppError> {
        let resources = ResourceLedger::from_file_data(file_data);
        self.resolve_with_resources(file_data, &resources)
    }

    pub fn resolve_with_resources(
        self,
        file_data: &DocumentData,
        resources: &ResourceLedger,
    ) -> Result<AppliedOperation, AppError> {
        match self {
            EditorCommand::SetCell {
                sheet_index,
                row,
                col,
                text,
            } => {
                require_sheet(file_data, sheet_index)?;
                let old_value = file_data.sheets[sheet_index]
                    .rows
                    .get(row)
                    .and_then(|row_data| row_data.get(col))
                    .cloned()
                    .unwrap_or(CellValue::Null);
                let new_value = parse_cell_text(&text);
                if old_value != new_value {
                    resources.validate_cell_changes(
                        file_data,
                        [(sheet_index, row, col, &old_value, &new_value)],
                    )?;
                }
                Ok(AppliedOperation::SetCell {
                    sheet_index,
                    row,
                    col,
                    old_value,
                    new_value,
                })
            }
            EditorCommand::SetCells { changes } => {
                if changes.is_empty() {
                    return Ok(AppliedOperation::SetCells {
                        changes: Vec::new(),
                    });
                }
                let mut resolved: Vec<ResolvedCellEdit> = Vec::with_capacity(changes.len());
                let mut positions: HashMap<(usize, usize, usize), usize> = HashMap::new();
                for change in changes {
                    require_sheet(file_data, change.sheet_index)?;
                    let key = (change.sheet_index, change.row, change.col);
                    let new_value = parse_cell_text(&change.text);
                    if let Some(index) = positions.get(&key) {
                        resolved[*index].new_value = new_value;
                    } else {
                        let old_value = file_data.sheets[change.sheet_index]
                            .rows
                            .get(change.row)
                            .and_then(|row_data| row_data.get(change.col))
                            .cloned()
                            .unwrap_or(CellValue::Null);
                        positions.insert(key, resolved.len());
                        resolved.push(ResolvedCellEdit {
                            sheet_index: change.sheet_index,
                            row: change.row,
                            col: change.col,
                            old_value,
                            new_value,
                        });
                    }
                }
                resolved.retain(|change| change.old_value != change.new_value);
                if !resolved.is_empty() {
                    resources.validate_cell_changes(
                        file_data,
                        resolved.iter().map(|change| {
                            (
                                change.sheet_index,
                                change.row,
                                change.col,
                                &change.old_value,
                                &change.new_value,
                            )
                        }),
                    )?;
                }
                Ok(AppliedOperation::SetCells { changes: resolved })
            }
            EditorCommand::AddRow {
                sheet_index,
                row_index,
            } => {
                let sheet = require_sheet(file_data, sheet_index)?;
                let extent = SheetMutationExtent::from_sheet(sheet);
                if row_index > extent.rows {
                    return Err(AppError::RowNotFound(row_index));
                }
                resources.validate_added_row(
                    sheet,
                    sheet.rows.len().max(row_index).saturating_add(1),
                    extent.columns,
                )?;
                Ok(AppliedOperation::AddRow {
                    sheet_index,
                    row_index,
                    row_data: vec![CellValue::Null; extent.columns],
                    row_height: None,
                })
            }
            EditorCommand::DeleteRow {
                sheet_index,
                row_index,
            } => {
                let sheet = require_sheet(file_data, sheet_index)?;
                let extent = SheetMutationExtent::from_sheet(sheet);
                if row_index >= extent.rows {
                    return Err(AppError::RowNotFound(row_index));
                }
                Ok(AppliedOperation::DeleteRow {
                    sheet_index,
                    row_index,
                })
            }
            EditorCommand::AddColumn {
                sheet_index,
                col_index,
            } => {
                let sheet = require_sheet(file_data, sheet_index)?;
                let extent = SheetMutationExtent::from_sheet(sheet);
                if col_index > extent.columns {
                    return Err(AppError::InvalidCellPosition {
                        row: 0,
                        col: col_index,
                    });
                }
                resources.validate_added_column(sheet, extent.rows, col_index)?;
                Ok(AppliedOperation::AddColumn {
                    sheet_index,
                    col_index,
                    col_data: vec![CellValue::Null; extent.rows],
                    column_width: None,
                })
            }
            EditorCommand::DeleteColumn {
                sheet_index,
                col_index,
            } => {
                let sheet = require_sheet(file_data, sheet_index)?;
                let extent = SheetMutationExtent::from_sheet(sheet);
                if col_index >= extent.columns {
                    return Err(AppError::InvalidCellPosition {
                        row: 0,
                        col: col_index,
                    });
                }
                Ok(AppliedOperation::DeleteColumn {
                    sheet_index,
                    col_index,
                })
            }
            EditorCommand::SetColumnWidth {
                sheet_index,
                col_index,
                width,
            } => {
                validate_column_width(width)?;
                let sheet = require_sheet(file_data, sheet_index)?;
                let extent = SheetMutationExtent::from_sheet(sheet);
                if col_index >= extent.resizable_columns() {
                    return Err(AppError::InvalidCellPosition {
                        row: 0,
                        col: col_index,
                    });
                }
                let old_width = sheet
                    .column_widths
                    .as_ref()
                    .and_then(|widths| widths.get(&col_index).copied());
                resources.validate_layout_change(old_width.is_some(), width.is_some())?;
                Ok(AppliedOperation::SetColumnWidth {
                    sheet_index,
                    col_index,
                    old_width,
                    new_width: width,
                })
            }
            EditorCommand::SetRowHeight {
                sheet_index,
                row_index,
                height,
            } => {
                validate_row_height(height)?;
                let sheet = require_sheet(file_data, sheet_index)?;
                let extent = SheetMutationExtent::from_sheet(sheet);
                if row_index >= extent.resizable_rows() {
                    return Err(AppError::RowNotFound(row_index));
                }
                let old_height = sheet
                    .row_heights
                    .as_ref()
                    .and_then(|heights| heights.get(&row_index).copied());
                resources.validate_layout_change(old_height.is_some(), height.is_some())?;
                Ok(AppliedOperation::SetRowHeight {
                    sheet_index,
                    row_index,
                    old_height,
                    new_height: height,
                })
            }
            EditorCommand::AddSheet { name } => {
                let sheet_index = file_data.sheets.len();
                let sheet_name = name
                    .filter(|name| !name.is_empty())
                    .unwrap_or_else(|| format!("Sheet{}", sheet_index + 1));
                validate_added_sheet(file_data, &sheet_name)?;
                Ok(AppliedOperation::AddSheet {
                    sheet_index,
                    name: sheet_name,
                    row_count: 5,
                    column_count: 5,
                })
            }
            EditorCommand::DeleteSheet { sheet_index } => {
                if file_data.sheets.len() <= 1 {
                    return Err(AppError::CannotDeleteLastSheet);
                }
                require_sheet(file_data, sheet_index)?;
                Ok(AppliedOperation::DeleteSheet { sheet_index })
            }
        }
    }
}

fn require_sheet(file_data: &DocumentData, sheet_index: usize) -> Result<&DocumentSheet, AppError> {
    file_data
        .sheets
        .get(sheet_index)
        .ok_or(AppError::InvalidSheetIndex(sheet_index))
}

struct SheetMutationExtent {
    rows: usize,
    columns: usize,
}

impl SheetMutationExtent {
    fn from_sheet(sheet: &DocumentSheet) -> Self {
        let value_rows = sheet.rows.len();
        let value_columns = sheet.rows.iter().map(Vec::len).max().unwrap_or(0);
        let merge_rows = sheet
            .merges
            .iter()
            .map(|merge| merge.end_row as usize + 1)
            .max()
            .unwrap_or(0);
        let merge_columns = sheet
            .merges
            .iter()
            .map(|merge| merge.end_col as usize + 1)
            .max()
            .unwrap_or(0);
        let layout_rows = sheet
            .row_heights
            .as_ref()
            .and_then(|heights| heights.keys().max().map(|index| index + 1))
            .unwrap_or(0);
        let layout_columns = sheet
            .column_widths
            .as_ref()
            .and_then(|widths| widths.keys().max().map(|index| index + 1))
            .unwrap_or(0);
        let rich = rich_projection_extent(&sheet.rich);

        Self {
            rows: value_rows.max(merge_rows).max(layout_rows).max(rich.rows),
            columns: value_columns
                .max(merge_columns)
                .max(layout_columns)
                .max(rich.columns),
        }
    }

    fn resizable_rows(&self) -> usize {
        self.rows.max(1)
    }

    fn resizable_columns(&self) -> usize {
        self.columns.max(1)
    }
}

fn rich_projection_extent(rich: &RichMetadata) -> SheetMutationExtent {
    let mut rows = 0;
    let mut columns = 0;

    for key in rich
        .cell_formats
        .keys()
        .chain(rich.cell_styles.keys())
        .chain(rich.hyperlinks.keys())
    {
        if let Some((row, col)) = parse_cell_key(key) {
            rows = rows.max(row + 1);
            columns = columns.max(col + 1);
        }
    }

    rows = rows.max(
        rich.hidden_rows
            .iter()
            .copied()
            .max()
            .map(|row| row + 1)
            .unwrap_or(0),
    );
    columns = columns.max(
        rich.hidden_columns
            .iter()
            .copied()
            .max()
            .map(|col| col + 1)
            .unwrap_or(0),
    );

    for drawing in &rich.drawings {
        rows = rows.max(
            (drawing
                .to_row
                .unwrap_or(drawing.from_row)
                .max(drawing.from_row) as usize)
                + 1,
        );
        columns = columns.max(
            (drawing
                .to_col
                .unwrap_or(drawing.from_col)
                .max(drawing.from_col) as usize)
                + 1,
        );
    }

    SheetMutationExtent { rows, columns }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document_data::{CellStyle, Drawing, DrawingKind, Hyperlink};
    fn file_data_with_rich(rich: RichMetadata) -> DocumentData {
        DocumentData {
            path: String::new(),
            file_name: "rich.xlsx".to_string(),
            sheets: vec![DocumentSheet {
                name: "Sheet1".to_string(),
                rows: Vec::new(),
                rich,
                ..Default::default()
            }],
        }
    }

    #[test]
    fn sheet_extent_includes_rich_cell_metadata() {
        let file_data = file_data_with_rich(RichMetadata {
            cell_styles: [(
                "E4".to_string(),
                CellStyle {
                    bold: Some(true),
                    ..Default::default()
                },
            )]
            .into_iter()
            .collect(),
            hyperlinks: [(
                "F5".to_string(),
                Hyperlink {
                    url: "https://example.com".to_string(),
                    tooltip: None,
                    location: false,
                },
            )]
            .into_iter()
            .collect(),
            ..Default::default()
        });

        assert!(
            matches!(
                EditorCommand::SetRowHeight {
                    sheet_index: 0,
                    row_index: 4,
                    height: Some(80),
                }
                .resolve(&file_data),
                Ok(AppliedOperation::SetRowHeight { row_index: 4, .. })
            ),
            "row extent should include hyperlink/style-only rows"
        );
        assert!(
            matches!(
                EditorCommand::SetColumnWidth {
                    sheet_index: 0,
                    col_index: 5,
                    width: Some(120),
                }
                .resolve(&file_data),
                Ok(AppliedOperation::SetColumnWidth { col_index: 5, .. })
            ),
            "column extent should include hyperlink/style-only columns"
        );
    }

    #[test]
    fn layout_mutations_reject_dimensions_outside_the_domain_policy() {
        let file_data = DocumentData {
            path: String::new(),
            file_name: "book.xlsx".to_string(),
            sheets: vec![DocumentSheet {
                rows: vec![vec![CellValue::Null]],
                ..Default::default()
            }],
        };

        assert!(matches!(
            EditorCommand::SetColumnWidth {
                sheet_index: 0,
                col_index: 0,
                width: Some(0),
            }
            .resolve(&file_data),
            Err(AppError::ResourceLimitExceeded(_))
        ));
        assert!(matches!(
            EditorCommand::SetRowHeight {
                sheet_index: 0,
                row_index: 0,
                height: Some(crate::document_layout_policy::MAX_ROW_HEIGHT_PX + 1),
            }
            .resolve(&file_data),
            Err(AppError::ResourceLimitExceeded(_))
        ));
    }

    #[test]
    fn sheet_extent_includes_hidden_rows_columns_and_drawings() {
        let file_data = file_data_with_rich(RichMetadata {
            hidden_rows: vec![9],
            hidden_columns: vec![7],
            drawings: vec![Drawing {
                kind: DrawingKind::Image,
                from_row: 11,
                from_col: 12,
                to_row: Some(14),
                to_col: Some(15),
            }],
            ..Default::default()
        });

        assert!(matches!(
            EditorCommand::DeleteRow {
                sheet_index: 0,
                row_index: 14,
            }
            .resolve(&file_data),
            Ok(AppliedOperation::DeleteRow { row_index: 14, .. })
        ));
        assert!(matches!(
            EditorCommand::DeleteColumn {
                sheet_index: 0,
                col_index: 15,
            }
            .resolve(&file_data),
            Ok(AppliedOperation::DeleteColumn { col_index: 15, .. })
        ));
    }
}
