use crate::document_data::{DocumentSheet, MergeRange, RichMetadata};
use std::collections::{HashMap, HashSet};

use crate::document::backing::document_body::BodyImageAsset;
use crate::document::backing::document_body::BodyStructureMemento;
use crate::document::backing::rich_projection::{
    RichProjectionScope, filter_rich_projection, restore_rich_projection_scope,
};
use crate::document_data::SheetImage;
use crate::document_resource_estimator::{
    estimate_cell_value_bytes, estimate_rich_metadata_bytes, estimate_sheet_data_bytes,
};
use crate::domain::{CellValue, DocumentCellChange, cell_key::parse_cell_key};

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
    Image(ImageMemento),
}

impl DocumentMementoSide {
    pub(crate) fn estimated_bytes(&self) -> usize {
        match self {
            DocumentMementoSide::Cells(memento) => memento.estimated_bytes(),
            DocumentMementoSide::Layout(memento) => memento.estimated_bytes(),
            DocumentMementoSide::Structure(memento) => memento.estimated_bytes(),
            DocumentMementoSide::Image(memento) => memento.estimated_bytes(),
        }
    }
}

pub(crate) struct ImageMemento {
    pub(crate) sheet_index: usize,
    pub(crate) image_id: String,
    pub(crate) image: Option<SheetImage>,
    pub(crate) asset: Option<BodyImageAsset>,
}

impl ImageMemento {
    fn estimated_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.image_id.len()
            + self
                .image
                .as_ref()
                .map(|image| image.media_id.len() + image.mime_type.len() + 192)
                .unwrap_or_default()
            + self
                .asset
                .as_ref()
                .map(|asset| asset.image_name.len() + asset.bytes.len())
                .unwrap_or_default()
    }
}

pub(crate) struct CellMemento {
    pub(crate) cells: Vec<DocumentCellChange>,
    pub(crate) sheet_shapes: Vec<SheetShapeMemento>,
    pub(crate) formula_capabilities_may_change: bool,
}

impl CellMemento {
    pub(crate) fn new(
        cells: Vec<DocumentCellChange>,
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
    projection: RichMetadata,
}

impl RichProjectionMemento {
    pub(crate) fn row_tail(source: &RichMetadata, row_index: usize) -> Self {
        Self {
            scope: RichProjectionScope::Rows { start: row_index },
            projection: filter_rich_projection(
                source,
                RichProjectionScope::Rows { start: row_index },
            ),
        }
    }

    pub(crate) fn column_tail(source: &RichMetadata, col_index: usize) -> Self {
        Self {
            scope: RichProjectionScope::Columns { start: col_index },
            projection: filter_rich_projection(
                source,
                RichProjectionScope::Columns { start: col_index },
            ),
        }
    }

    pub(crate) fn restore_into(&self, target: &mut RichMetadata) {
        restore_rich_projection_scope(target, self.scope, &self.projection);
    }

    fn estimated_bytes(&self) -> usize {
        std::mem::size_of::<Self>() + estimate_rich_metadata_bytes(&self.projection)
    }
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
    pub(crate) sheet: DocumentSheet,
}

impl ProjectionSheetSnapshot {
    fn estimated_bytes(&self) -> usize {
        std::mem::size_of::<Self>() + estimate_sheet_data_bytes(&self.sheet)
    }
}

pub(crate) fn protected_rich_cell_positions(sheet: &DocumentSheet) -> Vec<(usize, usize)> {
    let mut positions = Vec::new();
    let mut seen = HashSet::new();
    for key in sheet
        .rich
        .cell_formats
        .keys()
        .chain(sheet.rich.cell_styles.keys())
        .chain(sheet.rich.hyperlinks.keys())
    {
        if let Some((row, col)) = parse_cell_key(key)
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
    for image in &sheet.rich.images {
        let (from, to) = match &image.anchor {
            crate::document_data::ImageAnchor::OneCell { from, .. } => (from, None),
            crate::document_data::ImageAnchor::TwoCell { from, to } => (from, Some(to)),
        };
        push_unique_position_2d(
            &mut positions,
            &mut seen,
            from.row as usize,
            from.col as usize,
        );
        if let Some(to) = to {
            push_unique_position_2d(&mut positions, &mut seen, to.row as usize, to.col as usize);
        }
    }
    positions
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

fn estimate_sheet_cell_change_bytes(change: &DocumentCellChange) -> usize {
    std::mem::size_of::<DocumentCellChange>() + estimate_cell_value_bytes(&change.value)
}
