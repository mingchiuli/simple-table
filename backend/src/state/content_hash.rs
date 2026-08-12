use crate::document_data::{
    CellFormat, CellStyle, DocumentData, DocumentSheet, Drawing, FreezePane, Hyperlink, MergeRange,
    RichMetadata,
};
use sha2::{Digest, Sha256};

use std::collections::{BTreeMap, BTreeSet};

use crate::document::document_memento::DocumentMementoSide;
use crate::domain::{AppliedOperation, CellValue, DocumentCellChange};

pub type ContentHash = [u8; 32];

#[cfg(test)]
pub struct ContentFingerprint<'a> {
    sheets: Vec<SheetFingerprint<'a>>,
}

struct SheetFingerprint<'a> {
    name: &'a str,
    rows: &'a [Vec<CellValue>],
    merges: &'a [MergeRange],
    column_widths: Option<&'a std::collections::HashMap<usize, u32>>,
    row_heights: Option<&'a std::collections::HashMap<usize, u32>>,
    rich: &'a RichMetadata,
}

pub struct IncrementalContentFingerprint {
    hash: ContentHash,
    sheets: Vec<IncrementalSheetFingerprint>,
}

struct IncrementalSheetFingerprint {
    hash: ContentHash,
    row_lengths: Vec<usize>,
    min_row_length: usize,
    column_width_count: usize,
    row_height_count: usize,
}

#[derive(Clone)]
struct CellFingerprintChange {
    sheet_index: usize,
    row: usize,
    col: usize,
    old_value: CellValue,
    new_value: CellValue,
}

#[cfg(test)]
impl<'a> ContentFingerprint<'a> {
    pub fn from_file_data(file_data: &'a DocumentData) -> Self {
        Self {
            sheets: file_data
                .sheets
                .iter()
                .map(SheetFingerprint::from_sheet_data)
                .collect(),
        }
    }
}

impl<'a> SheetFingerprint<'a> {
    fn from_sheet_data(sheet: &'a DocumentSheet) -> Self {
        Self {
            name: &sheet.name,
            rows: &sheet.rows,
            merges: &sheet.merges,
            column_widths: sheet.column_widths.as_ref(),
            row_heights: sheet.row_heights.as_ref(),
            rich: &sheet.rich,
        }
    }
}

#[cfg(test)]
pub fn hash_content_fingerprint(fingerprint: &ContentFingerprint<'_>) -> ContentHash {
    let mut hash = contribution(TAG_VERSION, |_| {});
    xor_hash(
        &mut hash,
        &contribution(TAG_SHEET_COUNT, |hasher| {
            write_usize(hasher, fingerprint.sheets.len());
        }),
    );
    for (sheet_index, sheet) in fingerprint.sheets.iter().enumerate() {
        xor_hash(&mut hash, &hash_sheet_content(sheet_index, sheet));
    }
    hash
}

impl IncrementalContentFingerprint {
    pub fn from_file_data(file_data: &DocumentData) -> Self {
        let sheets: Vec<_> = file_data
            .sheets
            .iter()
            .enumerate()
            .map(|(sheet_index, sheet)| {
                IncrementalSheetFingerprint::from_sheet_data(sheet_index, sheet)
            })
            .collect();
        let mut hash = contribution(TAG_VERSION, |_| {});
        xor_hash(
            &mut hash,
            &contribution(TAG_SHEET_COUNT, |hasher| {
                write_usize(hasher, sheets.len());
            }),
        );
        for sheet in &sheets {
            xor_hash(&mut hash, &sheet.hash);
        }
        Self { hash, sheets }
    }

    pub fn hash(&self) -> ContentHash {
        self.hash
    }

    pub fn apply_operation(
        &mut self,
        operation: &AppliedOperation,
        formula_changes: &[DocumentCellChange],
        file_data: &DocumentData,
    ) {
        match operation {
            AppliedOperation::SetCell {
                sheet_index,
                row,
                col,
                old_value,
                new_value,
            } => self.apply_cell_changes(
                vec![CellFingerprintChange {
                    sheet_index: *sheet_index,
                    row: *row,
                    col: *col,
                    old_value: old_value.clone(),
                    new_value: new_value.clone(),
                }],
                file_data,
                &BTreeSet::new(),
            ),
            AppliedOperation::SetCells { changes } => self.apply_cell_changes(
                changes
                    .iter()
                    .map(|change| CellFingerprintChange {
                        sheet_index: change.sheet_index,
                        row: change.row,
                        col: change.col,
                        old_value: change.old_value.clone(),
                        new_value: change.new_value.clone(),
                    })
                    .collect(),
                file_data,
                &BTreeSet::new(),
            ),
            AppliedOperation::SetColumnWidth {
                sheet_index,
                col_index,
                old_width,
                new_width,
            } => self.apply_layout_change(
                *sheet_index,
                LayoutKind::Column,
                *col_index,
                *old_width,
                *new_width,
                file_data,
            ),
            AppliedOperation::SetRowHeight {
                sheet_index,
                row_index,
                old_height,
                new_height,
            } => self.apply_layout_change(
                *sheet_index,
                LayoutKind::Row,
                *row_index,
                *old_height,
                *new_height,
                file_data,
            ),
            AppliedOperation::AddRow { sheet_index, .. }
            | AppliedOperation::DeleteRow { sheet_index, .. }
            | AppliedOperation::AddColumn { sheet_index, .. }
            | AppliedOperation::DeleteColumn { sheet_index, .. } => {
                let mut affected_sheets: BTreeSet<_> = formula_changes
                    .iter()
                    .map(|change| change.sheet_index)
                    .collect();
                affected_sheets.insert(*sheet_index);
                for affected_sheet in affected_sheets {
                    self.rebuild_sheet(affected_sheet, file_data);
                }
            }
            AppliedOperation::AddSheet { .. } | AppliedOperation::DeleteSheet { .. } => {
                *self = Self::from_file_data(file_data);
            }
            AppliedOperation::InsertImage { sheet_index, .. }
            | AppliedOperation::UpdateImage { sheet_index, .. }
            | AppliedOperation::DeleteImage { sheet_index, .. } => {
                self.rebuild_sheet(*sheet_index, file_data);
            }
        }
    }

    pub fn apply_history_restore(
        &mut self,
        target: &DocumentMementoSide,
        rollback: &DocumentMementoSide,
        file_data: &DocumentData,
    ) {
        match (target, rollback) {
            (DocumentMementoSide::Cells(target), DocumentMementoSide::Cells(rollback)) => {
                let Some(changes) = history_cell_changes(&rollback.cells, &target.cells) else {
                    self.rebuild_changed_cell_sheets(&target.cells, &rollback.cells, file_data);
                    return;
                };
                let target_shapes: BTreeMap<_, _> = target
                    .sheet_shapes
                    .iter()
                    .map(|shape| (shape.sheet_index, &shape.row_lengths))
                    .collect();
                let rollback_shapes: BTreeMap<_, _> = rollback
                    .sheet_shapes
                    .iter()
                    .map(|shape| (shape.sheet_index, &shape.row_lengths))
                    .collect();
                if target_shapes.keys().collect::<Vec<_>>()
                    != rollback_shapes.keys().collect::<Vec<_>>()
                {
                    self.rebuild_changed_cell_sheets(&target.cells, &rollback.cells, file_data);
                    return;
                }
                let reconcile_shapes = target_shapes
                    .iter()
                    .filter_map(|(sheet_index, target)| {
                        (target != &rollback_shapes[sheet_index]).then_some(*sheet_index)
                    })
                    .collect();
                self.apply_cell_changes(changes, file_data, &reconcile_shapes);
            }
            (DocumentMementoSide::Layout(target), DocumentMementoSide::Layout(rollback)) => {
                if target.sheet_index != rollback.sheet_index
                    || target.column_widths.keys().collect::<BTreeSet<_>>()
                        != rollback.column_widths.keys().collect::<BTreeSet<_>>()
                    || target.row_heights.keys().collect::<BTreeSet<_>>()
                        != rollback.row_heights.keys().collect::<BTreeSet<_>>()
                {
                    self.rebuild_sheet(target.sheet_index, file_data);
                    return;
                }
                for (index, new_value) in &target.column_widths {
                    self.apply_layout_change(
                        target.sheet_index,
                        LayoutKind::Column,
                        *index,
                        rollback.column_widths[index],
                        *new_value,
                        file_data,
                    );
                }
                for (index, new_value) in &target.row_heights {
                    self.apply_layout_change(
                        target.sheet_index,
                        LayoutKind::Row,
                        *index,
                        rollback.row_heights[index],
                        *new_value,
                        file_data,
                    );
                }
            }
            (DocumentMementoSide::Structure(_), DocumentMementoSide::Structure(_)) => {
                *self = Self::from_file_data(file_data);
            }
            (DocumentMementoSide::Image(target), DocumentMementoSide::Image(_)) => {
                self.rebuild_sheet(target.sheet_index, file_data);
            }
            _ => *self = Self::from_file_data(file_data),
        }
    }

    fn apply_cell_changes(
        &mut self,
        changes: Vec<CellFingerprintChange>,
        file_data: &DocumentData,
        reconcile_shapes: &BTreeSet<usize>,
    ) {
        let mut changes_by_sheet = BTreeMap::<usize, Vec<CellFingerprintChange>>::new();
        for change in changes {
            changes_by_sheet
                .entry(change.sheet_index)
                .or_default()
                .push(change);
        }

        for (sheet_index, changes) in changes_by_sheet {
            let Some(sheet_data) = file_data.sheets.get(sheet_index) else {
                *self = Self::from_file_data(file_data);
                return;
            };
            let Some(sheet) = self.sheets.get_mut(sheet_index) else {
                *self = Self::from_file_data(file_data);
                return;
            };
            let old_sheet_hash = sheet.hash;
            let old_row_count = sheet.row_lengths.len();
            let new_row_count = sheet_data.rows.len();
            let changes_by_cell: BTreeMap<_, _> = changes
                .iter()
                .map(|change| ((change.row, change.col), change))
                .collect();
            let mut touched_rows: BTreeSet<usize> =
                changes.iter().map(|change| change.row).collect();
            touched_rows.extend(old_row_count.min(new_row_count)..old_row_count.max(new_row_count));
            let target_width = changes
                .iter()
                .map(|change| change.col.saturating_add(1))
                .max()
                .unwrap_or(0);
            let shape_changed = reconcile_shapes.contains(&sheet_index)
                || old_row_count != new_row_count
                || sheet.min_row_length < target_width;
            if shape_changed {
                touched_rows.extend(0..old_row_count.max(new_row_count));
            }

            if new_row_count > old_row_count {
                sheet.row_lengths.resize(new_row_count, 0);
            }

            if old_row_count != new_row_count {
                xor_hash(
                    &mut sheet.hash,
                    &row_count_contribution(sheet_index, old_row_count),
                );
                xor_hash(
                    &mut sheet.hash,
                    &row_count_contribution(sheet_index, new_row_count),
                );
            }

            for row in touched_rows {
                let old_len = sheet.row_lengths.get(row).copied().unwrap_or(0);
                let new_len = sheet_data.rows.get(row).map(Vec::len).unwrap_or(0);
                if row < old_row_count {
                    xor_hash(
                        &mut sheet.hash,
                        &row_length_contribution(sheet_index, row, old_len),
                    );
                }
                if row < new_row_count {
                    xor_hash(
                        &mut sheet.hash,
                        &row_length_contribution(sheet_index, row, new_len),
                    );
                }

                let common_len = old_len.min(new_len);
                for ((change_row, col), change) in
                    changes_by_cell.range((row, 0)..=(row, usize::MAX))
                {
                    debug_assert_eq!(*change_row, row);
                    if *col < common_len {
                        xor_hash(
                            &mut sheet.hash,
                            &cell_contribution(sheet_index, row, *col, &change.old_value),
                        );
                        xor_hash(
                            &mut sheet.hash,
                            &cell_contribution(sheet_index, row, *col, &change.new_value),
                        );
                    }
                }

                if new_len > old_len {
                    for col in old_len..new_len {
                        let value = &sheet_data.rows[row][col];
                        xor_hash(
                            &mut sheet.hash,
                            &cell_contribution(sheet_index, row, col, value),
                        );
                    }
                } else if old_len > new_len {
                    for col in new_len..old_len {
                        let old_value = changes_by_cell
                            .get(&(row, col))
                            .map(|change| &change.old_value)
                            .unwrap_or(&CellValue::Null);
                        xor_hash(
                            &mut sheet.hash,
                            &cell_contribution(sheet_index, row, col, old_value),
                        );
                    }
                }

                if row < new_row_count {
                    sheet.row_lengths[row] = new_len;
                }
            }

            sheet.row_lengths.truncate(new_row_count);
            if shape_changed {
                sheet.min_row_length = sheet
                    .row_lengths
                    .iter()
                    .copied()
                    .min()
                    .unwrap_or(usize::MAX);
            }
            xor_hash(&mut self.hash, &old_sheet_hash);
            xor_hash(&mut self.hash, &sheet.hash);
        }
    }

    fn apply_layout_change(
        &mut self,
        sheet_index: usize,
        kind: LayoutKind,
        index: usize,
        old_value: Option<u32>,
        new_value: Option<u32>,
        file_data: &DocumentData,
    ) {
        let Some(sheet) = self.sheets.get_mut(sheet_index) else {
            *self = Self::from_file_data(file_data);
            return;
        };
        let old_sheet_hash = sheet.hash;
        let (old_count, new_count) = match kind {
            LayoutKind::Column => (
                sheet.column_width_count,
                file_data.sheets[sheet_index]
                    .column_widths
                    .as_ref()
                    .map_or(0, |map| map.len()),
            ),
            LayoutKind::Row => (
                sheet.row_height_count,
                file_data.sheets[sheet_index]
                    .row_heights
                    .as_ref()
                    .map_or(0, |map| map.len()),
            ),
        };
        xor_hash(
            &mut sheet.hash,
            &layout_count_contribution(sheet_index, kind, old_count),
        );
        xor_hash(
            &mut sheet.hash,
            &layout_count_contribution(sheet_index, kind, new_count),
        );
        if let Some(value) = old_value {
            xor_hash(
                &mut sheet.hash,
                &layout_value_contribution(sheet_index, kind, index, value),
            );
        }
        if let Some(value) = new_value {
            xor_hash(
                &mut sheet.hash,
                &layout_value_contribution(sheet_index, kind, index, value),
            );
        }
        match kind {
            LayoutKind::Column => sheet.column_width_count = new_count,
            LayoutKind::Row => sheet.row_height_count = new_count,
        }
        xor_hash(&mut self.hash, &old_sheet_hash);
        xor_hash(&mut self.hash, &sheet.hash);
    }

    fn rebuild_sheet(&mut self, sheet_index: usize, file_data: &DocumentData) {
        let Some(sheet_data) = file_data.sheets.get(sheet_index) else {
            *self = Self::from_file_data(file_data);
            return;
        };
        let replacement = IncrementalSheetFingerprint::from_sheet_data(sheet_index, sheet_data);
        let Some(current) = self.sheets.get_mut(sheet_index) else {
            *self = Self::from_file_data(file_data);
            return;
        };
        xor_hash(&mut self.hash, &current.hash);
        xor_hash(&mut self.hash, &replacement.hash);
        *current = replacement;
    }

    fn rebuild_changed_cell_sheets(
        &mut self,
        target: &[DocumentCellChange],
        rollback: &[DocumentCellChange],
        file_data: &DocumentData,
    ) {
        let sheet_indexes: BTreeSet<_> = target
            .iter()
            .chain(rollback)
            .map(|change| change.sheet_index)
            .collect();
        for sheet_index in sheet_indexes {
            self.rebuild_sheet(sheet_index, file_data);
        }
    }
}

impl IncrementalSheetFingerprint {
    fn from_sheet_data(sheet_index: usize, sheet: &DocumentSheet) -> Self {
        let fingerprint = SheetFingerprint::from_sheet_data(sheet);
        let row_lengths: Vec<_> = sheet.rows.iter().map(Vec::len).collect();
        Self {
            hash: hash_sheet_content(sheet_index, &fingerprint),
            min_row_length: row_lengths.iter().copied().min().unwrap_or(usize::MAX),
            row_lengths,
            column_width_count: sheet.column_widths.as_ref().map_or(0, |map| map.len()),
            row_height_count: sheet.row_heights.as_ref().map_or(0, |map| map.len()),
        }
    }
}

#[derive(Clone, Copy)]
enum LayoutKind {
    Column,
    Row,
}

fn history_cell_changes(
    rollback: &[DocumentCellChange],
    target: &[DocumentCellChange],
) -> Option<Vec<CellFingerprintChange>> {
    let rollback: BTreeMap<_, _> = rollback
        .iter()
        .map(|change| ((change.sheet_index, change.row, change.col), &change.value))
        .collect();
    let target: BTreeMap<_, _> = target
        .iter()
        .map(|change| ((change.sheet_index, change.row, change.col), &change.value))
        .collect();
    if rollback.keys().collect::<Vec<_>>() != target.keys().collect::<Vec<_>>() {
        return None;
    }
    Some(
        rollback
            .into_iter()
            .map(
                |((sheet_index, row, col), old_value)| CellFingerprintChange {
                    sheet_index,
                    row,
                    col,
                    old_value: old_value.clone(),
                    new_value: target[&(sheet_index, row, col)].clone(),
                },
            )
            .collect(),
    )
}

fn hash_sheet_content(sheet_index: usize, sheet: &SheetFingerprint<'_>) -> ContentHash {
    let mut hash = contribution(TAG_SHEET_NAME, |hasher| {
        write_usize(hasher, sheet_index);
        write_str(hasher, sheet.name);
    });
    xor_hash(
        &mut hash,
        &row_count_contribution(sheet_index, sheet.rows.len()),
    );
    for (row_index, row) in sheet.rows.iter().enumerate() {
        xor_hash(
            &mut hash,
            &row_length_contribution(sheet_index, row_index, row.len()),
        );
        for (col_index, cell) in row.iter().enumerate() {
            xor_hash(
                &mut hash,
                &cell_contribution(sheet_index, row_index, col_index, cell),
            );
        }
    }
    xor_hash(
        &mut hash,
        &contribution(TAG_MERGE_COUNT, |hasher| {
            write_usize(hasher, sheet_index);
            write_usize(hasher, sheet.merges.len());
        }),
    );
    for (merge_index, merge) in sheet.merges.iter().enumerate() {
        xor_hash(
            &mut hash,
            &contribution(TAG_MERGE, |hasher| {
                write_usize(hasher, sheet_index);
                write_usize(hasher, merge_index);
                hash_merge_range(merge, hasher);
            }),
        );
    }
    hash_layout_map(
        sheet_index,
        LayoutKind::Column,
        sheet.column_widths,
        &mut hash,
    );
    hash_layout_map(sheet_index, LayoutKind::Row, sheet.row_heights, &mut hash);
    hash_rich_metadata(sheet_index, sheet.rich, &mut hash);
    hash
}

fn hash_rich_metadata(sheet_index: usize, rich: &RichMetadata, hash: &mut ContentHash) {
    xor_hash(
        hash,
        &contribution(TAG_CELL_FORMAT_COUNT, |hasher| {
            write_usize(hasher, sheet_index);
            write_usize(hasher, rich.cell_formats.len());
        }),
    );
    for (cell, format) in &rich.cell_formats {
        xor_hash(
            hash,
            &contribution(TAG_CELL_FORMAT, |hasher| {
                write_usize(hasher, sheet_index);
                write_str(hasher, cell);
                hash_cell_format(format, hasher);
            }),
        );
    }

    xor_hash(
        hash,
        &contribution(TAG_CELL_STYLE_COUNT, |hasher| {
            write_usize(hasher, sheet_index);
            write_usize(hasher, rich.cell_styles.len());
        }),
    );
    for (cell, style) in &rich.cell_styles {
        xor_hash(
            hash,
            &contribution(TAG_CELL_STYLE, |hasher| {
                write_usize(hasher, sheet_index);
                write_str(hasher, cell);
                hash_cell_style(style, hasher);
            }),
        );
    }

    hash_hidden_indexes(
        sheet_index,
        &rich.hidden_rows,
        TAG_HIDDEN_ROW_COUNT,
        TAG_HIDDEN_ROW,
        hash,
    );
    hash_hidden_indexes(
        sheet_index,
        &rich.hidden_columns,
        TAG_HIDDEN_COLUMN_COUNT,
        TAG_HIDDEN_COLUMN,
        hash,
    );

    xor_hash(
        hash,
        &contribution(TAG_FREEZE_PANE, |hasher| {
            write_usize(hasher, sheet_index);
            write_optional_freeze_pane(rich.freeze_pane.as_ref(), hasher);
        }),
    );

    xor_hash(
        hash,
        &contribution(TAG_HYPERLINK_COUNT, |hasher| {
            write_usize(hasher, sheet_index);
            write_usize(hasher, rich.hyperlinks.len());
        }),
    );
    for (cell, hyperlink) in &rich.hyperlinks {
        xor_hash(
            hash,
            &contribution(TAG_HYPERLINK, |hasher| {
                write_usize(hasher, sheet_index);
                write_str(hasher, cell);
                hash_hyperlink(hyperlink, hasher);
            }),
        );
    }

    xor_hash(
        hash,
        &contribution(TAG_DRAWING_COUNT, |hasher| {
            write_usize(hasher, sheet_index);
            write_usize(hasher, rich.drawings.len());
        }),
    );
    for (drawing_index, drawing) in rich.drawings.iter().enumerate() {
        xor_hash(
            hash,
            &contribution(TAG_DRAWING, |hasher| {
                write_usize(hasher, sheet_index);
                write_usize(hasher, drawing_index);
                hash_drawing(drawing, hasher);
            }),
        );
    }
    xor_hash(
        hash,
        &contribution(TAG_IMAGE_COUNT, |hasher| {
            write_usize(hasher, sheet_index);
            write_usize(hasher, rich.images.len());
        }),
    );
    for (image_index, image) in rich.images.iter().enumerate() {
        xor_hash(
            hash,
            &contribution(TAG_IMAGE, |hasher| {
                write_usize(hasher, sheet_index);
                write_usize(hasher, image_index);
                write_str(hasher, &image.media_id);
                hash_image_anchor(&image.anchor, hasher);
            }),
        );
    }
}

fn hash_cell_format(format: &CellFormat, hasher: &mut Sha256) {
    write_optional_str(hasher, format.number_format.as_deref());
    write_optional_str(hasher, format.style_id.as_deref());
}

fn hash_cell_style(style: &CellStyle, hasher: &mut Sha256) {
    write_optional_str(hasher, style.font_color.as_deref());
    write_optional_str(hasher, style.background_color.as_deref());
    write_optional_bool(hasher, style.bold);
    write_optional_bool(hasher, style.italic);
    write_optional_str(hasher, style.horizontal_align.as_deref());
    write_optional_str(hasher, style.vertical_align.as_deref());
    write_optional_str(hasher, style.number_format.as_deref());
}

fn hash_hidden_indexes(
    sheet_index: usize,
    indexes: &[usize],
    count_tag: u8,
    value_tag: u8,
    hash: &mut ContentHash,
) {
    let indexes: BTreeSet<_> = indexes.iter().copied().collect();
    xor_hash(
        hash,
        &contribution(count_tag, |hasher| {
            write_usize(hasher, sheet_index);
            write_usize(hasher, indexes.len());
        }),
    );
    for index in indexes {
        xor_hash(
            hash,
            &contribution(value_tag, |hasher| {
                write_usize(hasher, sheet_index);
                write_usize(hasher, index);
            }),
        );
    }
}

fn write_optional_freeze_pane(value: Option<&FreezePane>, hasher: &mut Sha256) {
    let Some(value) = value else {
        write_tag(hasher, 0);
        return;
    };
    write_tag(hasher, 1);
    write_str(hasher, &value.top_left_cell);
    write_f64(hasher, value.horizontal_split);
    write_f64(hasher, value.vertical_split);
    write_str(hasher, &value.active_pane);
    write_str(hasher, &value.state);
}

fn hash_hyperlink(hyperlink: &Hyperlink, hasher: &mut Sha256) {
    write_str(hasher, &hyperlink.url);
    write_optional_str(hasher, hyperlink.tooltip.as_deref());
    write_bool(hasher, hyperlink.location);
}

fn hash_drawing(drawing: &Drawing, hasher: &mut Sha256) {
    write_tag(hasher, 1);
    write_u32(hasher, drawing.from_row);
    write_u32(hasher, drawing.from_col);
    write_optional_u32(hasher, drawing.to_row);
    write_optional_u32(hasher, drawing.to_col);
}

fn hash_image_anchor(anchor: &crate::document_data::ImageAnchor, hasher: &mut Sha256) {
    match anchor {
        crate::document_data::ImageAnchor::OneCell {
            from,
            width_emu,
            height_emu,
        } => {
            write_tag(hasher, 0);
            hash_image_marker(from, hasher);
            hasher.update(width_emu.to_le_bytes());
            hasher.update(height_emu.to_le_bytes());
        }
        crate::document_data::ImageAnchor::TwoCell { from, to } => {
            write_tag(hasher, 1);
            hash_image_marker(from, hasher);
            hash_image_marker(to, hasher);
        }
    }
}

fn hash_image_marker(marker: &crate::document_data::ImageMarker, hasher: &mut Sha256) {
    write_u32(hasher, marker.row);
    write_u32(hasher, marker.col);
    hasher.update(marker.row_offset_emu.to_le_bytes());
    hasher.update(marker.col_offset_emu.to_le_bytes());
}

fn hash_cell_value(cell: &CellValue, hasher: &mut Sha256) {
    match cell {
        CellValue::Null => {
            write_tag(hasher, 0);
        }
        CellValue::String(value) => {
            write_tag(hasher, 1);
            write_str(hasher, value);
        }
        CellValue::Number(value) => {
            write_tag(hasher, 2);
            write_str(hasher, &value.to_string());
        }
        CellValue::Boolean(value) => {
            write_tag(hasher, 3);
            hasher.update([u8::from(*value)]);
        }
        CellValue::Formula { formula, .. } => {
            write_tag(hasher, 4);
            write_str(hasher, formula);
        }
    }
}

fn hash_merge_range(merge: &MergeRange, hasher: &mut Sha256) {
    write_u32(hasher, merge.start_row);
    write_u16(hasher, merge.start_col);
    write_u32(hasher, merge.end_row);
    write_u16(hasher, merge.end_col);
}

fn hash_layout_map(
    sheet_index: usize,
    kind: LayoutKind,
    map: Option<&std::collections::HashMap<usize, u32>>,
    hash: &mut ContentHash,
) {
    let count = map.map_or(0, |map| map.len());
    xor_hash(hash, &layout_count_contribution(sheet_index, kind, count));
    if let Some(map) = map {
        for (index, value) in map {
            xor_hash(
                hash,
                &layout_value_contribution(sheet_index, kind, *index, *value),
            );
        }
    }
}

const TAG_VERSION: u8 = 0;
const TAG_SHEET_COUNT: u8 = 1;
const TAG_SHEET_NAME: u8 = 2;
const TAG_ROW_COUNT: u8 = 3;
const TAG_ROW_LENGTH: u8 = 4;
const TAG_CELL: u8 = 5;
const TAG_MERGE_COUNT: u8 = 6;
const TAG_MERGE: u8 = 7;
const TAG_COLUMN_WIDTH_COUNT: u8 = 8;
const TAG_COLUMN_WIDTH: u8 = 9;
const TAG_ROW_HEIGHT_COUNT: u8 = 10;
const TAG_ROW_HEIGHT: u8 = 11;
const TAG_CELL_FORMAT_COUNT: u8 = 12;
const TAG_CELL_FORMAT: u8 = 13;
const TAG_CELL_STYLE_COUNT: u8 = 14;
const TAG_CELL_STYLE: u8 = 15;
const TAG_HIDDEN_ROW_COUNT: u8 = 16;
const TAG_HIDDEN_ROW: u8 = 17;
const TAG_HIDDEN_COLUMN_COUNT: u8 = 18;
const TAG_HIDDEN_COLUMN: u8 = 19;
const TAG_FREEZE_PANE: u8 = 20;
const TAG_HYPERLINK_COUNT: u8 = 21;
const TAG_HYPERLINK: u8 = 22;
const TAG_DRAWING_COUNT: u8 = 23;
const TAG_DRAWING: u8 = 24;
const TAG_IMAGE_COUNT: u8 = 25;
const TAG_IMAGE: u8 = 26;

fn contribution(tag: u8, write: impl FnOnce(&mut Sha256)) -> ContentHash {
    let mut hasher = Sha256::new();
    write_tag(&mut hasher, tag);
    write(&mut hasher);
    hasher.finalize().into()
}

fn row_count_contribution(sheet_index: usize, row_count: usize) -> ContentHash {
    contribution(TAG_ROW_COUNT, |hasher| {
        write_usize(hasher, sheet_index);
        write_usize(hasher, row_count);
    })
}

fn row_length_contribution(sheet_index: usize, row_index: usize, row_length: usize) -> ContentHash {
    contribution(TAG_ROW_LENGTH, |hasher| {
        write_usize(hasher, sheet_index);
        write_usize(hasher, row_index);
        write_usize(hasher, row_length);
    })
}

fn cell_contribution(
    sheet_index: usize,
    row_index: usize,
    col_index: usize,
    cell: &CellValue,
) -> ContentHash {
    contribution(TAG_CELL, |hasher| {
        write_usize(hasher, sheet_index);
        write_usize(hasher, row_index);
        write_usize(hasher, col_index);
        hash_cell_value(cell, hasher);
    })
}

fn layout_count_contribution(sheet_index: usize, kind: LayoutKind, count: usize) -> ContentHash {
    contribution(
        match kind {
            LayoutKind::Column => TAG_COLUMN_WIDTH_COUNT,
            LayoutKind::Row => TAG_ROW_HEIGHT_COUNT,
        },
        |hasher| {
            write_usize(hasher, sheet_index);
            write_usize(hasher, count);
        },
    )
}

fn layout_value_contribution(
    sheet_index: usize,
    kind: LayoutKind,
    index: usize,
    value: u32,
) -> ContentHash {
    contribution(
        match kind {
            LayoutKind::Column => TAG_COLUMN_WIDTH,
            LayoutKind::Row => TAG_ROW_HEIGHT,
        },
        |hasher| {
            write_usize(hasher, sheet_index);
            write_usize(hasher, index);
            write_u32(hasher, value);
        },
    )
}

fn xor_hash(target: &mut ContentHash, contribution: &ContentHash) {
    for (target, contribution) in target.iter_mut().zip(contribution) {
        *target ^= contribution;
    }
}

fn write_tag(hasher: &mut Sha256, tag: u8) {
    hasher.update([tag]);
}

fn write_str(hasher: &mut Sha256, value: &str) {
    write_usize(hasher, value.len());
    hasher.update(value.as_bytes());
}

fn write_optional_str(hasher: &mut Sha256, value: Option<&str>) {
    match value {
        Some(value) => {
            write_tag(hasher, 1);
            write_str(hasher, value);
        }
        None => write_tag(hasher, 0),
    }
}

fn write_bool(hasher: &mut Sha256, value: bool) {
    hasher.update([u8::from(value)]);
}

fn write_optional_bool(hasher: &mut Sha256, value: Option<bool>) {
    match value {
        Some(value) => {
            write_tag(hasher, 1);
            write_bool(hasher, value);
        }
        None => write_tag(hasher, 0),
    }
}

fn write_optional_u32(hasher: &mut Sha256, value: Option<u32>) {
    match value {
        Some(value) => {
            write_tag(hasher, 1);
            write_u32(hasher, value);
        }
        None => write_tag(hasher, 0),
    }
}

fn write_f64(hasher: &mut Sha256, value: f64) {
    hasher.update(value.to_bits().to_le_bytes());
}

fn write_usize(hasher: &mut Sha256, value: usize) {
    hasher.update(value.to_le_bytes());
}

fn write_u32(hasher: &mut Sha256, value: u32) {
    hasher.update(value.to_le_bytes());
}

fn write_u16(hasher: &mut Sha256, value: u16) {
    hasher.update(value.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{ContentFingerprint, hash_content_fingerprint};
    use crate::document_data::{
        CellFormat, CellStyle, DocumentData, DocumentSheet, Drawing, DrawingKind, FreezePane,
        Hyperlink,
    };
    use crate::domain::CellValue;

    fn file_data() -> DocumentData {
        DocumentData {
            path: "/tmp/source.xlsx".to_string(),
            file_name: "source.xlsx".to_string(),
            sheets: vec![DocumentSheet {
                name: "Sheet1".to_string(),
                rows: vec![vec![CellValue::String("a".to_string())]],
                merges: vec![],
                ..Default::default()
            }],
        }
    }

    fn hash_file_content(file_data: &DocumentData) -> super::ContentHash {
        hash_content_fingerprint(&ContentFingerprint::from_file_data(file_data))
    }

    #[test]
    fn hash_ignores_file_identity() {
        let original = file_data();
        let mut changed_layout = original.clone();
        changed_layout.path = "/tmp/renamed.xlsx".to_string();
        changed_layout.file_name = "renamed.xlsx".to_string();

        assert_eq!(
            hash_file_content(&original),
            hash_file_content(&changed_layout)
        );
    }

    #[test]
    fn hash_changes_when_persisted_layout_changes() {
        let original = file_data();
        let mut changed_layout = original.clone();
        changed_layout.sheets[0].column_widths = Some(HashMap::from([(0, 240)]));
        changed_layout.sheets[0].row_heights = Some(HashMap::from([(0, 96)]));

        assert_ne!(
            hash_file_content(&original),
            hash_file_content(&changed_layout)
        );
    }

    #[test]
    fn hash_changes_when_persisted_rich_metadata_changes() {
        let original = file_data();
        let original_hash = hash_file_content(&original);

        let mut variants = Vec::new();
        let mut cell_format = original.clone();
        cell_format.sheets[0].rich.cell_formats.insert(
            "A1".to_string(),
            CellFormat {
                number_format: Some("0.00".to_string()),
                style_id: Some("3".to_string()),
            },
        );
        variants.push(cell_format);

        let mut cell_style = original.clone();
        cell_style.sheets[0].rich.cell_styles.insert(
            "A1".to_string(),
            CellStyle {
                bold: Some(true),
                background_color: Some("#ffffff".to_string()),
                ..Default::default()
            },
        );
        variants.push(cell_style);

        let mut hidden = original.clone();
        hidden.sheets[0].rich.hidden_rows.push(2);
        hidden.sheets[0].rich.hidden_columns.push(3);
        variants.push(hidden);

        let mut frozen = original.clone();
        frozen.sheets[0].rich.freeze_pane = Some(FreezePane {
            top_left_cell: "B2".to_string(),
            horizontal_split: 1.0,
            vertical_split: 1.0,
            active_pane: "bottomRight".to_string(),
            state: "frozen".to_string(),
        });
        variants.push(frozen);

        let mut linked = original.clone();
        linked.sheets[0].rich.hyperlinks.insert(
            "A1".to_string(),
            Hyperlink {
                url: "https://example.com".to_string(),
                tooltip: Some("Example".to_string()),
                location: false,
            },
        );
        variants.push(linked);

        let mut drawn = original;
        drawn.sheets[0].rich.drawings.push(Drawing {
            kind: DrawingKind::Chart,
            from_row: 0,
            from_col: 0,
            to_row: Some(2),
            to_col: Some(2),
        });
        variants.push(drawn);

        for variant in variants {
            assert_ne!(original_hash, hash_file_content(&variant));
        }
    }

    #[test]
    fn hash_ignores_derived_rich_metadata_flags_and_hidden_index_order() {
        let mut original = file_data();
        original.sheets[0].rich.hidden_rows = vec![3, 1];
        original.sheets[0].rich.hidden_columns = vec![4, 2];

        let mut changed = original.clone();
        changed.sheets[0].rich.hidden_rows.reverse();
        changed.sheets[0].rich.hidden_columns.reverse();
        changed.sheets[0].rich.has_more_drawings = true;
        changed.sheets[0].rich.has_style_metadata = true;
        changed.sheets[0].rich.has_hyperlinks = true;
        changed.sheets[0].rich.has_freeze_pane = true;

        assert_eq!(hash_file_content(&original), hash_file_content(&changed));
    }

    #[test]
    fn hash_changes_when_saved_content_changes() {
        let original = file_data();
        let mut changed_content = original.clone();
        changed_content.sheets[0].rows[0][0] = CellValue::String("b".to_string());

        assert_ne!(
            hash_file_content(&original),
            hash_file_content(&changed_content)
        );
    }

    #[test]
    fn formula_hash_uses_formula_text_not_cached_value() {
        let mut original = file_data();
        original.sheets[0].rows[0][0] = CellValue::formula("=A2+1", CellValue::Number(1.into()));

        let mut changed_cache = original.clone();
        changed_cache.sheets[0].rows[0][0] =
            CellValue::formula("=A2+1", CellValue::Number(2.into()));

        let mut changed_formula = original.clone();
        changed_formula.sheets[0].rows[0][0] =
            CellValue::formula("=A2+2", CellValue::Number(1.into()));

        assert_eq!(
            hash_file_content(&original),
            hash_file_content(&changed_cache)
        );
        assert_ne!(
            hash_file_content(&original),
            hash_file_content(&changed_formula)
        );
    }
}
