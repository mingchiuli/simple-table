use sha2::{Digest, Sha256};

use crate::domain::EditorCommand;
use crate::error::AppError;

pub(crate) type MutationFingerprint = [u8; 32];

#[derive(Debug, Clone)]
pub(crate) enum MutationIntent {
    Undo,
    Redo,
    Execute(EditorCommand),
}

impl MutationIntent {
    pub(crate) fn fingerprint(&self, base_revision: u64) -> Result<MutationFingerprint, AppError> {
        let mut fingerprint = FingerprintWriter::default();
        fingerprint.write_u64(base_revision);
        match self {
            Self::Undo => fingerprint.write_tag(0),
            Self::Redo => fingerprint.write_tag(1),
            Self::Execute(command) => fingerprint.write_editor_command(command)?,
        }
        Ok(fingerprint.finish())
    }
}

#[derive(Default)]
struct FingerprintWriter(Sha256);

impl FingerprintWriter {
    fn write_editor_command(&mut self, command: &EditorCommand) -> Result<(), AppError> {
        match command {
            EditorCommand::SetCell {
                sheet_index,
                row,
                col,
                text,
            } => {
                self.write_tag(2);
                self.write_index(*sheet_index)?;
                self.write_index(*row)?;
                self.write_index(*col)?;
                self.write_text(text)?;
            }
            EditorCommand::SetCells { changes } => {
                self.write_tag(3);
                self.write_index(changes.len())?;
                for edit in changes {
                    self.write_index(edit.sheet_index)?;
                    self.write_index(edit.row)?;
                    self.write_index(edit.col)?;
                    self.write_text(&edit.text)?;
                }
            }
            EditorCommand::AddRow {
                sheet_index,
                row_index,
            } => {
                self.write_tag(4);
                self.write_index(*sheet_index)?;
                self.write_index(*row_index)?;
            }
            EditorCommand::DeleteRow {
                sheet_index,
                row_index,
            } => {
                self.write_tag(5);
                self.write_index(*sheet_index)?;
                self.write_index(*row_index)?;
            }
            EditorCommand::AddColumn {
                sheet_index,
                col_index,
            } => {
                self.write_tag(6);
                self.write_index(*sheet_index)?;
                self.write_index(*col_index)?;
            }
            EditorCommand::DeleteColumn {
                sheet_index,
                col_index,
            } => {
                self.write_tag(7);
                self.write_index(*sheet_index)?;
                self.write_index(*col_index)?;
            }
            EditorCommand::SetColumnWidth {
                sheet_index,
                col_index,
                width,
            } => {
                self.write_tag(8);
                self.write_index(*sheet_index)?;
                self.write_index(*col_index)?;
                self.write_optional_u32(*width);
            }
            EditorCommand::SetRowHeight {
                sheet_index,
                row_index,
                height,
            } => {
                self.write_tag(9);
                self.write_index(*sheet_index)?;
                self.write_index(*row_index)?;
                self.write_optional_u32(*height);
            }
            EditorCommand::AddSheet { name } => {
                self.write_tag(10);
                self.write_optional_text(name.as_deref())?;
            }
            EditorCommand::DeleteSheet { sheet_index } => {
                self.write_tag(11);
                self.write_index(*sheet_index)?;
            }
            EditorCommand::InsertImage {
                sheet_index, image, ..
            } => {
                self.write_tag(12);
                self.write_index(*sheet_index)?;
                self.write_text(&image.id)?;
                self.write_text(&image.media_id)?;
                self.write_image_anchor(&image.anchor)?;
            }
            EditorCommand::UpdateImage {
                sheet_index,
                image_id,
                anchor,
            } => {
                self.write_tag(13);
                self.write_index(*sheet_index)?;
                self.write_text(image_id)?;
                self.write_image_anchor(anchor)?;
            }
            EditorCommand::DeleteImage {
                sheet_index,
                image_id,
            } => {
                self.write_tag(14);
                self.write_index(*sheet_index)?;
                self.write_text(image_id)?;
            }
        }
        Ok(())
    }

    fn write_tag(&mut self, tag: u8) {
        self.0.update([tag]);
    }

    fn write_u64(&mut self, value: u64) {
        self.0.update(value.to_le_bytes());
    }

    fn write_index(&mut self, value: usize) -> Result<(), AppError> {
        self.write_u64(u64::try_from(value).map_err(|_| {
            AppError::ResourceLimitExceeded("mutation index exceeds u64 range".to_string())
        })?);
        Ok(())
    }

    fn write_text(&mut self, value: &str) -> Result<(), AppError> {
        self.write_index(value.len())?;
        self.0.update(value.as_bytes());
        Ok(())
    }

    fn write_optional_text(&mut self, value: Option<&str>) -> Result<(), AppError> {
        match value {
            Some(value) => {
                self.write_tag(1);
                self.write_text(value)?;
            }
            None => self.write_tag(0),
        }
        Ok(())
    }

    fn write_optional_u32(&mut self, value: Option<u32>) {
        match value {
            Some(value) => {
                self.write_tag(1);
                self.0.update(value.to_le_bytes());
            }
            None => self.write_tag(0),
        }
    }

    fn write_image_anchor(
        &mut self,
        anchor: &crate::document_data::ImageAnchor,
    ) -> Result<(), AppError> {
        use crate::document_data::ImageAnchor;
        match anchor {
            ImageAnchor::OneCell {
                from,
                width_emu,
                height_emu,
            } => {
                self.write_tag(0);
                self.write_image_marker(from);
                self.0.update(width_emu.to_le_bytes());
                self.0.update(height_emu.to_le_bytes());
            }
            ImageAnchor::TwoCell { from, to } => {
                self.write_tag(1);
                self.write_image_marker(from);
                self.write_image_marker(to);
            }
        }
        Ok(())
    }

    fn write_image_marker(&mut self, marker: &crate::document_data::ImageMarker) {
        self.write_u64(u64::from(marker.row));
        self.write_u64(u64::from(marker.col));
        self.0.update(marker.row_offset_emu.to_le_bytes());
        self.0.update(marker.col_offset_emu.to_le_bytes());
    }

    fn finish(self) -> MutationFingerprint {
        self.0.finalize().into()
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
