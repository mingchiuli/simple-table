use crate::application::replay::{Fingerprint, FingerprintWriter};
use crate::domain::EditorCommand;
use crate::error::AppError;

pub(crate) type MutationFingerprint = Fingerprint;

#[derive(Debug, Clone)]
pub(crate) enum MutationIntent {
    Undo,
    Redo,
    Execute(EditorCommand),
}

impl MutationIntent {
    pub(crate) fn fingerprint(&self, base_revision: u64) -> Result<MutationFingerprint, AppError> {
        let mut fingerprint = MutationFingerprintWriter::default();
        fingerprint.0.write_u64(base_revision);
        match self {
            Self::Undo => fingerprint.0.write_tag(0),
            Self::Redo => fingerprint.0.write_tag(1),
            Self::Execute(command) => fingerprint.write_editor_command(command)?,
        }
        Ok(fingerprint.finish())
    }
}

#[derive(Default)]
struct MutationFingerprintWriter(FingerprintWriter);

impl MutationFingerprintWriter {
    fn write_editor_command(&mut self, command: &EditorCommand) -> Result<(), AppError> {
        match command {
            EditorCommand::SetCell {
                sheet_index,
                row,
                col,
                text,
            } => {
                self.0.write_tag(2);
                self.0.write_index(*sheet_index)?;
                self.0.write_index(*row)?;
                self.0.write_index(*col)?;
                self.0.write_text(text);
            }
            EditorCommand::SetCells { changes } => {
                self.0.write_tag(3);
                self.0.write_index(changes.len())?;
                for edit in changes {
                    self.0.write_index(edit.sheet_index)?;
                    self.0.write_index(edit.row)?;
                    self.0.write_index(edit.col)?;
                    self.0.write_text(&edit.text);
                }
            }
            EditorCommand::AddRow {
                sheet_index,
                row_index,
            } => {
                self.0.write_tag(4);
                self.0.write_index(*sheet_index)?;
                self.0.write_index(*row_index)?;
            }
            EditorCommand::DeleteRow {
                sheet_index,
                row_index,
            } => {
                self.0.write_tag(5);
                self.0.write_index(*sheet_index)?;
                self.0.write_index(*row_index)?;
            }
            EditorCommand::AddColumn {
                sheet_index,
                col_index,
            } => {
                self.0.write_tag(6);
                self.0.write_index(*sheet_index)?;
                self.0.write_index(*col_index)?;
            }
            EditorCommand::DeleteColumn {
                sheet_index,
                col_index,
            } => {
                self.0.write_tag(7);
                self.0.write_index(*sheet_index)?;
                self.0.write_index(*col_index)?;
            }
            EditorCommand::SetColumnWidth {
                sheet_index,
                col_index,
                width,
            } => {
                self.0.write_tag(8);
                self.0.write_index(*sheet_index)?;
                self.0.write_index(*col_index)?;
                self.write_optional_u32(*width);
            }
            EditorCommand::SetRowHeight {
                sheet_index,
                row_index,
                height,
            } => {
                self.0.write_tag(9);
                self.0.write_index(*sheet_index)?;
                self.0.write_index(*row_index)?;
                self.write_optional_u32(*height);
            }
            EditorCommand::AddSheet { name } => {
                self.0.write_tag(10);
                self.write_optional_text(name.as_deref())?;
            }
            EditorCommand::DeleteSheet { sheet_index } => {
                self.0.write_tag(11);
                self.0.write_index(*sheet_index)?;
            }
            EditorCommand::InsertImage {
                sheet_index, image, ..
            } => {
                self.0.write_tag(12);
                self.0.write_index(*sheet_index)?;
                self.0.write_text(&image.id);
                self.0.write_text(&image.media_id);
                self.write_image_anchor(&image.anchor)?;
            }
            EditorCommand::UpdateImage {
                sheet_index,
                image_id,
                anchor,
            } => {
                self.0.write_tag(13);
                self.0.write_index(*sheet_index)?;
                self.0.write_text(image_id);
                self.write_image_anchor(anchor)?;
            }
            EditorCommand::DeleteImage {
                sheet_index,
                image_id,
            } => {
                self.0.write_tag(14);
                self.0.write_index(*sheet_index)?;
                self.0.write_text(image_id);
            }
            EditorCommand::SortRows {
                sheet_index,
                anchor_row,
                anchor_col,
                direction,
            } => {
                self.0.write_tag(15);
                self.0.write_index(*sheet_index)?;
                self.0.write_index(*anchor_row)?;
                self.0.write_index(*anchor_col)?;
                self.0.write_tag(match direction {
                    crate::domain::SortDirection::Ascending => 0,
                    crate::domain::SortDirection::Descending => 1,
                });
            }
            EditorCommand::SetFilter {
                sheet_index,
                anchor_row,
                col,
                operator,
                value,
            } => {
                self.0.write_tag(16);
                self.0.write_index(*sheet_index)?;
                self.0.write_index(*anchor_row)?;
                self.0.write_index(*col)?;
                self.0.write_tag(match operator {
                    crate::domain::FilterOperator::Equals => 0,
                    crate::domain::FilterOperator::NotEquals => 1,
                    crate::domain::FilterOperator::Contains => 2,
                    crate::domain::FilterOperator::Blank => 3,
                    crate::domain::FilterOperator::NotBlank => 4,
                });
                self.0.write_text(value);
            }
            EditorCommand::ClearFilter { sheet_index, col } => {
                self.0.write_tag(17);
                self.0.write_index(*sheet_index)?;
                match col {
                    Some(col) => {
                        self.0.write_tag(1);
                        self.0.write_index(*col)?;
                    }
                    None => self.0.write_tag(0),
                }
            }
        }
        Ok(())
    }

    fn write_optional_text(&mut self, value: Option<&str>) -> Result<(), AppError> {
        match value {
            Some(value) => {
                self.0.write_tag(1);
                self.0.write_text(value);
            }
            None => self.0.write_tag(0),
        }
        Ok(())
    }

    fn write_optional_u32(&mut self, value: Option<u32>) {
        match value {
            Some(value) => {
                self.0.write_tag(1);
                self.0.write_u32(value);
            }
            None => self.0.write_tag(0),
        }
    }

    fn write_image_anchor(
        &mut self,
        anchor: &crate::document::data::ImageAnchor,
    ) -> Result<(), AppError> {
        use crate::document::data::ImageAnchor;
        match anchor {
            ImageAnchor::OneCell {
                from,
                width_emu,
                height_emu,
            } => {
                self.0.write_tag(0);
                self.write_image_marker(from);
                self.0.write_i64(*width_emu);
                self.0.write_i64(*height_emu);
            }
            ImageAnchor::TwoCell { from, to } => {
                self.0.write_tag(1);
                self.write_image_marker(from);
                self.write_image_marker(to);
            }
        }
        Ok(())
    }

    fn write_image_marker(&mut self, marker: &crate::document::data::ImageMarker) {
        self.0.write_u64(u64::from(marker.row));
        self.0.write_u64(u64::from(marker.col));
        self.0.write_i32(marker.row_offset_emu);
        self.0.write_i32(marker.col_offset_emu);
    }

    fn finish(self) -> MutationFingerprint {
        self.0.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::CellEditInput;

    fn execute(command: EditorCommand) -> MutationIntent {
        MutationIntent::Execute(command)
    }

    #[test]
    fn fingerprints_cover_every_intent_variant_and_command_field() {
        let intents = vec![
            MutationIntent::Undo,
            MutationIntent::Redo,
            execute(EditorCommand::SetCell {
                sheet_index: 1,
                row: 2,
                col: 3,
                text: "value".to_string(),
            }),
            execute(EditorCommand::SetCells {
                changes: vec![CellEditInput {
                    sheet_index: 1,
                    row: 2,
                    col: 3,
                    text: "value".to_string(),
                }],
            }),
            execute(EditorCommand::AddRow {
                sheet_index: 1,
                row_index: 2,
            }),
            execute(EditorCommand::DeleteRow {
                sheet_index: 1,
                row_index: 2,
            }),
            execute(EditorCommand::AddColumn {
                sheet_index: 1,
                col_index: 2,
            }),
            execute(EditorCommand::DeleteColumn {
                sheet_index: 1,
                col_index: 2,
            }),
            execute(EditorCommand::SetColumnWidth {
                sheet_index: 1,
                col_index: 2,
                width: Some(120),
            }),
            execute(EditorCommand::SetRowHeight {
                sheet_index: 1,
                row_index: 2,
                height: Some(40),
            }),
            execute(EditorCommand::AddSheet {
                name: Some("Sheet".to_string()),
            }),
            execute(EditorCommand::DeleteSheet { sheet_index: 1 }),
        ];

        let fingerprints = intents
            .iter()
            .map(|intent| intent.fingerprint(7).expect("fingerprint"))
            .collect::<std::collections::HashSet<_>>();

        assert_eq!(fingerprints.len(), intents.len());
        assert_ne!(
            intents[2].fingerprint(7).expect("current revision"),
            intents[2].fingerprint(8).expect("next revision")
        );
    }

    #[test]
    fn add_sheet_name_participates_in_the_fingerprint() {
        let unnamed = execute(EditorCommand::AddSheet { name: None });
        let named = execute(EditorCommand::AddSheet {
            name: Some("Sheet".to_string()),
        });

        assert_ne!(
            unnamed.fingerprint(0).expect("unnamed fingerprint"),
            named.fingerprint(0).expect("named fingerprint")
        );
    }
}
