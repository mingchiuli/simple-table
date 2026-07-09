use std::collections::HashMap;

use crate::types::{DrawingProjection, ReadOnlyRichProjection};

#[derive(Clone, Copy)]
pub(crate) enum RichProjectionScope {
    Rows { start: usize },
    Columns { start: usize },
}

impl RichProjectionScope {
    pub(crate) fn contains_cell(self, row: usize, col: usize) -> bool {
        match self {
            Self::Rows { start } => row >= start,
            Self::Columns { start } => col >= start,
        }
    }

    pub(crate) fn contains_row(self, row: usize) -> bool {
        match self {
            Self::Rows { start } => row >= start,
            Self::Columns { .. } => false,
        }
    }

    pub(crate) fn contains_column(self, col: usize) -> bool {
        match self {
            Self::Rows { .. } => false,
            Self::Columns { start } => col >= start,
        }
    }

    pub(crate) fn contains_drawing(self, drawing: &DrawingProjection) -> bool {
        match self {
            Self::Rows { start } => drawing_row_scope_affected(drawing, start),
            Self::Columns { start } => drawing_column_scope_affected(drawing, start),
        }
    }

    pub(crate) fn contains_freeze_pane(self, projection: &ReadOnlyRichProjection) -> bool {
        projection
            .freeze_pane
            .as_ref()
            .and_then(|pane| parse_cell_key(&pane.top_left_cell))
            .is_some_and(|(row, col)| self.contains_cell(row, col))
    }
}

pub(crate) fn filter_rich_projection(
    source: &ReadOnlyRichProjection,
    scope: RichProjectionScope,
) -> ReadOnlyRichProjection {
    ReadOnlyRichProjection {
        cell_formats: filter_cell_projection_map(&source.cell_formats, scope),
        cell_styles: filter_cell_projection_map(&source.cell_styles, scope),
        hidden_rows: source
            .hidden_rows
            .iter()
            .copied()
            .filter(|row| scope.contains_row(*row))
            .collect(),
        hidden_columns: source
            .hidden_columns
            .iter()
            .copied()
            .filter(|column| scope.contains_column(*column))
            .collect(),
        freeze_pane: source
            .freeze_pane
            .clone()
            .filter(|_| scope.contains_freeze_pane(source)),
        hyperlinks: filter_cell_projection_map(&source.hyperlinks, scope),
        drawings: source
            .drawings
            .iter()
            .filter(|drawing| scope.contains_drawing(drawing))
            .cloned()
            .collect(),
        has_more_drawings: source.has_more_drawings,
        has_style_metadata: source.has_style_metadata,
        has_hyperlinks: source.has_hyperlinks,
        has_freeze_pane: source.has_freeze_pane,
    }
}

pub(crate) fn restore_rich_projection_scope(
    target: &mut ReadOnlyRichProjection,
    scope: RichProjectionScope,
    projection: &ReadOnlyRichProjection,
) {
    target
        .cell_formats
        .retain(|key, _| !cell_key_matches(key, |row, col| scope.contains_cell(row, col)));
    target
        .cell_styles
        .retain(|key, _| !cell_key_matches(key, |row, col| scope.contains_cell(row, col)));
    target
        .hyperlinks
        .retain(|key, _| !cell_key_matches(key, |row, col| scope.contains_cell(row, col)));
    target
        .drawings
        .retain(|drawing| !scope.contains_drawing(drawing));
    target.hidden_rows.retain(|row| !scope.contains_row(*row));
    target
        .hidden_columns
        .retain(|column| !scope.contains_column(*column));

    if target
        .freeze_pane
        .as_ref()
        .and_then(|pane| parse_cell_key(&pane.top_left_cell))
        .is_none_or(|(row, col)| scope.contains_cell(row, col))
    {
        target.freeze_pane = projection.freeze_pane.clone();
    }

    target.cell_formats.extend(projection.cell_formats.clone());
    target.cell_styles.extend(projection.cell_styles.clone());
    target.hyperlinks.extend(projection.hyperlinks.clone());
    target.drawings.extend(projection.drawings.clone());
    target
        .hidden_rows
        .extend(projection.hidden_rows.iter().copied());
    target
        .hidden_columns
        .extend(projection.hidden_columns.iter().copied());
    target.hidden_rows.sort_unstable();
    target.hidden_rows.dedup();
    target.hidden_columns.sort_unstable();
    target.hidden_columns.dedup();

    target.has_more_drawings |= projection.has_more_drawings;
    target.has_style_metadata = !target.cell_formats.is_empty() || !target.cell_styles.is_empty();
    target.has_hyperlinks = !target.hyperlinks.is_empty();
    target.has_freeze_pane = target.freeze_pane.is_some();
}

pub(crate) fn parse_cell_key(key: &str) -> Option<(usize, usize)> {
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

pub(crate) fn drawing_row_scope_affected(drawing: &DrawingProjection, row_index: usize) -> bool {
    drawing.from_row as usize >= row_index
        || drawing
            .to_row
            .is_some_and(|to_row| to_row as usize >= row_index)
}

pub(crate) fn drawing_column_scope_affected(drawing: &DrawingProjection, col_index: usize) -> bool {
    drawing.from_col as usize >= col_index
        || drawing
            .to_col
            .is_some_and(|to_col| to_col as usize >= col_index)
}

fn filter_cell_projection_map<T: Clone>(
    source: &HashMap<String, T>,
    scope: RichProjectionScope,
) -> HashMap<String, T> {
    source
        .iter()
        .filter(|(key, _)| cell_key_matches(key, |row, col| scope.contains_cell(row, col)))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

fn cell_key_matches(key: &str, matches: impl Fn(usize, usize) -> bool) -> bool {
    parse_cell_key(key).is_some_and(|(row, col)| matches(row, col))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::types::{CellStyleProjection, FreezePaneProjection};

    use super::*;

    fn freeze(top_left_cell: &str) -> FreezePaneProjection {
        FreezePaneProjection {
            top_left_cell: top_left_cell.to_string(),
            horizontal_split: 1.0,
            vertical_split: 1.0,
            active_pane: "BottomRight".to_string(),
            state: "Frozen".to_string(),
        }
    }

    #[test]
    fn restore_row_scope_keeps_unaffected_freeze_pane() {
        let mut target = ReadOnlyRichProjection {
            cell_styles: HashMap::from([(
                "A1".to_string(),
                CellStyleProjection {
                    bold: Some(true),
                    ..Default::default()
                },
            )]),
            freeze_pane: Some(freeze("A1")),
            ..Default::default()
        };
        let projection = ReadOnlyRichProjection {
            cell_styles: HashMap::from([(
                "A3".to_string(),
                CellStyleProjection {
                    italic: Some(true),
                    ..Default::default()
                },
            )]),
            freeze_pane: None,
            ..Default::default()
        };

        restore_rich_projection_scope(
            &mut target,
            RichProjectionScope::Rows { start: 2 },
            &projection,
        );

        assert!(target.cell_styles.contains_key("A1"));
        assert!(target.cell_styles.contains_key("A3"));
        assert_eq!(
            target
                .freeze_pane
                .as_ref()
                .map(|pane| pane.top_left_cell.as_str()),
            Some("A1")
        );
    }

    #[test]
    fn restore_column_scope_replaces_affected_freeze_pane() {
        let mut target = ReadOnlyRichProjection {
            freeze_pane: Some(freeze("C1")),
            ..Default::default()
        };
        let projection = ReadOnlyRichProjection {
            freeze_pane: Some(freeze("D1")),
            ..Default::default()
        };

        restore_rich_projection_scope(
            &mut target,
            RichProjectionScope::Columns { start: 2 },
            &projection,
        );

        assert_eq!(
            target
                .freeze_pane
                .as_ref()
                .map(|pane| pane.top_left_cell.as_str()),
            Some("D1")
        );
    }
}
