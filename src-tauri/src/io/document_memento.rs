use std::collections::{HashMap, HashSet};

use crate::io::document_body::BodyStructureMemento;
use crate::types::{
    CellFormatProjection, CellStyleProjection, CellValue, DrawingProjection, FreezePaneProjection,
    HyperlinkProjection, MergeRange, ReadOnlyRichProjection, SheetCellChange, SheetData,
};

pub(crate) struct DocumentMemento {
    pub(crate) before: DocumentMementoSide,
    pub(crate) after: DocumentMementoSide,
}

impl DocumentMemento {
    pub(crate) fn new(before: DocumentMementoSide, after: DocumentMementoSide) -> Self {
        Self { before, after }
    }

    pub(crate) fn estimated_bytes(&self) -> usize {
        self.before.estimated_bytes() + self.after.estimated_bytes()
    }
}

pub(crate) enum DocumentMementoSide {
    Cells(CellMemento),
    Layout(LayoutMemento),
    Structure(Box<StructureMemento>),
}

impl DocumentMementoSide {
    pub(crate) fn estimated_bytes(&self) -> usize {
        match self {
            DocumentMementoSide::Cells(memento) => memento.estimated_bytes(),
            DocumentMementoSide::Layout(memento) => memento.estimated_bytes(),
            DocumentMementoSide::Structure(memento) => memento.estimated_bytes(),
        }
    }
}

pub(crate) struct CellMemento {
    pub(crate) cells: Vec<SheetCellChange>,
    pub(crate) sheet_shapes: Vec<SheetShapeMemento>,
    pub(crate) formula_capabilities_may_change: bool,
}

impl CellMemento {
    pub(crate) fn new(
        cells: Vec<SheetCellChange>,
        sheet_shapes: Vec<SheetShapeMemento>,
        formula_capabilities_may_change: bool,
    ) -> Self {
        Self {
            cells,
            sheet_shapes,
            formula_capabilities_may_change,
        }
    }

    fn estimated_bytes(&self) -> usize {
        self.cells
            .iter()
            .map(estimate_sheet_cell_change_bytes)
            .sum::<usize>()
            + self
                .sheet_shapes
                .iter()
                .map(SheetShapeMemento::estimated_bytes)
                .sum::<usize>()
    }
}

pub(crate) struct SheetShapeMemento {
    pub(crate) sheet_index: usize,
    pub(crate) row_lengths: Vec<usize>,
    pub(crate) protected_cells: Vec<(usize, usize)>,
}

impl SheetShapeMemento {
    fn estimated_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.row_lengths.len() * std::mem::size_of::<usize>()
            + self.protected_cells.len() * std::mem::size_of::<(usize, usize)>()
    }
}

pub(crate) struct LayoutMemento {
    pub(crate) sheet_index: usize,
    pub(crate) column_widths: HashMap<usize, Option<u32>>,
    pub(crate) row_heights: HashMap<usize, Option<u32>>,
}

impl LayoutMemento {
    pub(crate) fn new(
        sheet_index: usize,
        column_widths: HashMap<usize, Option<u32>>,
        row_heights: HashMap<usize, Option<u32>>,
    ) -> Self {
        Self {
            sheet_index,
            column_widths,
            row_heights,
        }
    }

    fn estimated_bytes(&self) -> usize {
        std::mem::size_of::<Self>() + (self.column_widths.len() + self.row_heights.len()) * 32
    }
}

pub(crate) struct StructureMemento {
    pub(crate) projection: FileStructureMemento,
    pub(crate) body: BodyStructureMemento,
}

impl StructureMemento {
    pub(crate) fn new(projection: FileStructureMemento, body: BodyStructureMemento) -> Self {
        Self { projection, body }
    }

    fn estimated_bytes(&self) -> usize {
        self.projection.estimated_bytes() + self.body.estimated_bytes()
    }
}

pub(crate) enum FileStructureMemento {
    Empty { sheet_count: usize },
    Row(RowStructureMemento),
    Column(ColumnStructureMemento),
    Sheets(SheetTailMemento),
}

impl FileStructureMemento {
    pub(crate) fn empty(sheet_count: usize) -> Self {
        Self::Empty { sheet_count }
    }

    fn estimated_bytes(&self) -> usize {
        match self {
            Self::Empty { .. } => std::mem::size_of::<Self>(),
            Self::Row(memento) => memento.estimated_bytes(),
            Self::Column(memento) => memento.estimated_bytes(),
            Self::Sheets(memento) => memento.estimated_bytes(),
        }
    }
}

pub(crate) struct RowStructureMemento {
    pub(crate) sheet_index: usize,
    pub(crate) row_index: usize,
    pub(crate) row_count: usize,
    pub(crate) row: Option<Vec<CellValue>>,
    pub(crate) merges: Vec<MergeRange>,
    pub(crate) row_heights: Option<HashMap<usize, u32>>,
    pub(crate) rich: RichProjectionMemento,
}

impl RowStructureMemento {
    fn estimated_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self
                .row
                .as_ref()
                .map(|row| {
                    std::mem::size_of::<Vec<CellValue>>()
                        + row.iter().map(estimate_cell_value_bytes).sum::<usize>()
                })
                .unwrap_or_default()
            + self.merges.len() * std::mem::size_of::<MergeRange>()
            + self
                .row_heights
                .as_ref()
                .map(|heights| heights.len() * 24)
                .unwrap_or_default()
            + self.rich.estimated_bytes()
    }
}

pub(crate) struct ColumnStructureMemento {
    pub(crate) sheet_index: usize,
    pub(crate) col_index: usize,
    pub(crate) row_lengths: Vec<usize>,
    pub(crate) values: Vec<Option<CellValue>>,
    pub(crate) merges: Vec<MergeRange>,
    pub(crate) column_widths: Option<HashMap<usize, u32>>,
    pub(crate) rich: RichProjectionMemento,
}

impl ColumnStructureMemento {
    fn estimated_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.row_lengths.len() * std::mem::size_of::<usize>()
            + self
                .values
                .iter()
                .flatten()
                .map(estimate_cell_value_bytes)
                .sum::<usize>()
            + self.merges.len() * std::mem::size_of::<MergeRange>()
            + self
                .column_widths
                .as_ref()
                .map(|widths| widths.len() * 24)
                .unwrap_or_default()
            + self.rich.estimated_bytes()
    }
}

pub(crate) struct RichProjectionMemento {
    scope: RichProjectionScope,
    projection: ReadOnlyRichProjection,
}

impl RichProjectionMemento {
    pub(crate) fn row_tail(source: &ReadOnlyRichProjection, row_index: usize) -> Self {
        Self {
            scope: RichProjectionScope::Rows { start: row_index },
            projection: filter_rich_projection(
                source,
                |row, _| row >= row_index,
                |row| row >= row_index,
                |_| false,
                |drawing| drawing_row_scope_affected(drawing, row_index),
            ),
        }
    }

    pub(crate) fn column_tail(source: &ReadOnlyRichProjection, col_index: usize) -> Self {
        Self {
            scope: RichProjectionScope::Columns { start: col_index },
            projection: filter_rich_projection(
                source,
                |_, col| col >= col_index,
                |_| false,
                |col| col >= col_index,
                |drawing| drawing_column_scope_affected(drawing, col_index),
            ),
        }
    }

    pub(crate) fn restore_into(&self, target: &mut ReadOnlyRichProjection) {
        match self.scope {
            RichProjectionScope::Rows { start } => {
                target
                    .cell_formats
                    .retain(|key, _| !cell_key_matches(key, |row, _| row >= start));
                target
                    .cell_styles
                    .retain(|key, _| !cell_key_matches(key, |row, _| row >= start));
                target
                    .drawings
                    .retain(|drawing| !drawing_row_scope_affected(drawing, start));
                target.hidden_rows.retain(|row| *row < start);
                target
                    .hyperlinks
                    .retain(|key, _| !cell_key_matches(key, |row, _| row >= start));
            }
            RichProjectionScope::Columns { start } => {
                target
                    .cell_formats
                    .retain(|key, _| !cell_key_matches(key, |_, col| col >= start));
                target
                    .cell_styles
                    .retain(|key, _| !cell_key_matches(key, |_, col| col >= start));
                target
                    .drawings
                    .retain(|drawing| !drawing_column_scope_affected(drawing, start));
                target.hidden_columns.retain(|col| *col < start);
                target
                    .hyperlinks
                    .retain(|key, _| !cell_key_matches(key, |_, col| col >= start));
            }
        }

        target
            .cell_formats
            .extend(self.projection.cell_formats.clone());
        target
            .cell_styles
            .extend(self.projection.cell_styles.clone());
        target
            .hidden_rows
            .extend(self.projection.hidden_rows.iter().copied());
        target.hidden_rows.sort_unstable();
        target.hidden_rows.dedup();
        target
            .hidden_columns
            .extend(self.projection.hidden_columns.iter().copied());
        target.hidden_columns.sort_unstable();
        target.hidden_columns.dedup();
        target.freeze_pane = self.projection.freeze_pane.clone();
        target.hyperlinks.extend(self.projection.hyperlinks.clone());
        target.drawings.extend(self.projection.drawings.clone());
        target.has_more_drawings = self.projection.has_more_drawings;
    }

    fn estimated_bytes(&self) -> usize {
        std::mem::size_of::<Self>() + estimate_sheet_rich_projection_bytes(&self.projection)
    }
}

#[derive(Clone, Copy)]
enum RichProjectionScope {
    Rows { start: usize },
    Columns { start: usize },
}

pub(crate) struct SheetTailMemento {
    pub(crate) sheet_count: usize,
    pub(crate) truncate_from: usize,
    pub(crate) sheets: Vec<ProjectionSheetSnapshot>,
}

impl SheetTailMemento {
    fn estimated_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self
                .sheets
                .iter()
                .map(ProjectionSheetSnapshot::estimated_bytes)
                .sum::<usize>()
    }
}

pub(crate) struct ProjectionSheetSnapshot {
    pub(crate) sheet_index: usize,
    pub(crate) sheet: SheetData,
}

impl ProjectionSheetSnapshot {
    fn estimated_bytes(&self) -> usize {
        std::mem::size_of::<Self>() + estimate_sheet_data_bytes(&self.sheet)
    }
}

#[derive(Clone, Copy)]
pub(crate) enum MementoSide {
    Before,
    After,
}

pub(crate) fn protected_rich_cell_positions(sheet: &SheetData) -> Vec<(usize, usize)> {
    let mut positions = Vec::new();
    let mut seen = HashSet::new();
    for key in sheet
        .rich
        .cell_formats
        .keys()
        .chain(sheet.rich.cell_styles.keys())
        .chain(sheet.rich.hyperlinks.keys())
    {
        if let Some((row, col)) = parse_projection_cell_key(key)
            && seen.insert((row, col))
        {
            positions.push((row, col));
        }
    }
    for drawing in &sheet.rich.drawings {
        push_unique_position_2d(
            &mut positions,
            &mut seen,
            drawing.from_row as usize,
            drawing.from_col as usize,
        );
        if let (Some(row), Some(col)) = (drawing.to_row, drawing.to_col) {
            push_unique_position_2d(&mut positions, &mut seen, row as usize, col as usize);
        }
    }
    positions
}

fn filter_rich_projection(
    source: &ReadOnlyRichProjection,
    cell_matches: impl Fn(usize, usize) -> bool,
    row_matches: impl Fn(usize) -> bool,
    column_matches: impl Fn(usize) -> bool,
    drawing_matches: impl Fn(&DrawingProjection) -> bool,
) -> ReadOnlyRichProjection {
    let cell_formats = filter_cell_projection_map(&source.cell_formats, &cell_matches);
    let cell_styles = filter_cell_projection_map(&source.cell_styles, &cell_matches);
    let freeze_pane = source.freeze_pane.clone();
    let hyperlinks = filter_cell_projection_map(&source.hyperlinks, &cell_matches);
    let drawings: Vec<DrawingProjection> = source
        .drawings
        .iter()
        .filter(|drawing| drawing_matches(drawing))
        .cloned()
        .collect();

    ReadOnlyRichProjection {
        has_style_metadata: !cell_formats.is_empty() || !cell_styles.is_empty(),
        has_hyperlinks: !hyperlinks.is_empty(),
        has_freeze_pane: freeze_pane.is_some(),
        cell_formats,
        cell_styles,
        hidden_rows: source
            .hidden_rows
            .iter()
            .copied()
            .filter(|row| row_matches(*row))
            .collect(),
        hidden_columns: source
            .hidden_columns
            .iter()
            .copied()
            .filter(|column| column_matches(*column))
            .collect(),
        freeze_pane,
        hyperlinks,
        drawings,
        has_more_drawings: source.has_more_drawings,
    }
}

fn filter_cell_projection_map<T: Clone>(
    source: &HashMap<String, T>,
    cell_matches: &impl Fn(usize, usize) -> bool,
) -> HashMap<String, T> {
    source
        .iter()
        .filter(|(key, _)| cell_key_matches(key, cell_matches))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

fn cell_key_matches(key: &str, matches: impl Fn(usize, usize) -> bool) -> bool {
    parse_projection_cell_key(key).is_some_and(|(row, col)| matches(row, col))
}

fn parse_projection_cell_key(key: &str) -> Option<(usize, usize)> {
    let mut col = 0usize;
    let mut row = 0usize;
    let mut saw_digit = false;
    for byte in key.bytes() {
        if byte.is_ascii_alphabetic() && !saw_digit {
            col = col
                .checked_mul(26)?
                .checked_add(usize::from(byte.to_ascii_uppercase() - b'A' + 1))?;
        } else if byte.is_ascii_digit() {
            saw_digit = true;
            row = row.checked_mul(10)?.checked_add(usize::from(byte - b'0'))?;
        } else {
            return None;
        }
    }
    (col > 0 && row > 0).then_some((row - 1, col - 1))
}

fn drawing_row_scope_affected(drawing: &DrawingProjection, row_index: usize) -> bool {
    drawing.from_row as usize >= row_index
        || drawing
            .to_row
            .is_some_and(|to_row| to_row as usize >= row_index)
}

fn drawing_column_scope_affected(drawing: &DrawingProjection, col_index: usize) -> bool {
    drawing.from_col as usize >= col_index
        || drawing
            .to_col
            .is_some_and(|to_col| to_col as usize >= col_index)
}

fn push_unique_position_2d(
    positions: &mut Vec<(usize, usize)>,
    seen: &mut HashSet<(usize, usize)>,
    row: usize,
    col: usize,
) {
    if seen.insert((row, col)) {
        positions.push((row, col));
    }
}

fn estimate_sheet_cell_change_bytes(change: &SheetCellChange) -> usize {
    std::mem::size_of::<SheetCellChange>() + estimate_cell_value_bytes(&change.value)
}

fn estimate_sheet_data_bytes(sheet: &SheetData) -> usize {
    std::mem::size_of::<SheetData>()
        + sheet.name.len()
        + sheet
            .rows
            .iter()
            .map(|row| {
                std::mem::size_of::<Vec<CellValue>>()
                    + row.iter().map(estimate_cell_value_bytes).sum::<usize>()
            })
            .sum::<usize>()
        + sheet.merges.len() * std::mem::size_of::<MergeRange>()
        + sheet
            .column_widths
            .as_ref()
            .map(|widths| widths.len() * 24)
            .unwrap_or_default()
        + sheet
            .row_heights
            .as_ref()
            .map(|heights| heights.len() * 24)
            .unwrap_or_default()
        + estimate_sheet_rich_projection_bytes(&sheet.rich)
}

fn estimate_sheet_rich_projection_bytes(rich: &ReadOnlyRichProjection) -> usize {
    std::mem::size_of::<ReadOnlyRichProjection>()
        + rich
            .cell_formats
            .iter()
            .map(|(cell, format)| cell.len() + estimate_cell_format_projection_bytes(format))
            .sum::<usize>()
        + rich
            .cell_styles
            .iter()
            .map(|(cell, style)| cell.len() + estimate_cell_style_projection_bytes(style))
            .sum::<usize>()
        + rich.hidden_rows.len() * std::mem::size_of::<usize>()
        + rich.hidden_columns.len() * std::mem::size_of::<usize>()
        + rich
            .freeze_pane
            .as_ref()
            .map(estimate_freeze_pane_projection_bytes)
            .unwrap_or_default()
        + rich
            .hyperlinks
            .iter()
            .map(|(cell, hyperlink)| cell.len() + estimate_hyperlink_projection_bytes(hyperlink))
            .sum::<usize>()
        + rich.drawings.len() * std::mem::size_of::<DrawingProjection>()
}

fn estimate_cell_format_projection_bytes(format: &CellFormatProjection) -> usize {
    std::mem::size_of::<CellFormatProjection>()
        + format
            .number_format
            .as_ref()
            .map(String::len)
            .unwrap_or_default()
        + format
            .style_id
            .as_ref()
            .map(String::len)
            .unwrap_or_default()
}

fn estimate_cell_style_projection_bytes(style: &CellStyleProjection) -> usize {
    style
        .font_color
        .as_ref()
        .map(String::len)
        .unwrap_or_default()
        + style
            .background_color
            .as_ref()
            .map(String::len)
            .unwrap_or_default()
        + style
            .horizontal_align
            .as_ref()
            .map(String::len)
            .unwrap_or_default()
        + style
            .vertical_align
            .as_ref()
            .map(String::len)
            .unwrap_or_default()
        + style
            .number_format
            .as_ref()
            .map(String::len)
            .unwrap_or_default()
        + std::mem::size_of::<CellStyleProjection>()
}

fn estimate_freeze_pane_projection_bytes(freeze_pane: &FreezePaneProjection) -> usize {
    std::mem::size_of::<FreezePaneProjection>()
        + freeze_pane.top_left_cell.len()
        + freeze_pane.active_pane.len()
        + freeze_pane.state.len()
}

fn estimate_hyperlink_projection_bytes(hyperlink: &HyperlinkProjection) -> usize {
    std::mem::size_of::<HyperlinkProjection>()
        + hyperlink.url.len()
        + hyperlink
            .tooltip
            .as_ref()
            .map(String::len)
            .unwrap_or_default()
}

fn estimate_cell_value_bytes(cell: &CellValue) -> usize {
    match cell {
        CellValue::Null | CellValue::Boolean(_) => std::mem::size_of::<CellValue>(),
        CellValue::String(value) => std::mem::size_of::<CellValue>() + value.len(),
        CellValue::Number(value) => std::mem::size_of::<CellValue>() + value.to_string().len(),
        CellValue::Formula {
            formula,
            cached_value,
            error,
        } => {
            std::mem::size_of::<CellValue>()
                + formula.len()
                + estimate_cell_value_bytes(cached_value)
                + error.as_ref().map(String::len).unwrap_or_default()
        }
    }
}
