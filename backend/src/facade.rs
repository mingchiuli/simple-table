use std::collections::HashMap;

use crate::protocol::{
    AppErrorDto, CellEdit, EditorReply, EditorRequest, EditorResponse, ImageAnchorDto,
    ImageMarkerDto, SheetImageDto,
};

use crate::application::document_codec_port::OpenDocumentSource;
use crate::application::document_file_workflow::DocumentOpenSourcePort;
use crate::application::{
    document_query_service, document_save_service, document_service, editor_command_service,
};
use crate::document::region_metadata_index::DocumentRegion;
use crate::domain::{EditorCommand, SearchScope};
use crate::error::AppError;
use crate::runtime::ApplicationRuntime;

struct MemoryOpenSource(OpenDocumentSource);

impl DocumentOpenSourcePort for MemoryOpenSource {
    fn read(self: Box<Self>) -> Result<OpenDocumentSource, AppError> {
        Ok(self.0)
    }
}

#[derive(Default)]
pub struct CoreFacade {
    runtime: ApplicationRuntime,
    pending_saves: HashMap<String, document_save_service::PreparedDocumentSave>,
}

impl CoreFacade {
    pub fn execute(&mut self, request: EditorRequest) -> EditorResponse {
        self.execute_inner(request).map_err(AppErrorDto::from)
    }

    fn execute_inner(&mut self, request: EditorRequest) -> Result<EditorReply, AppError> {
        match request {
            EditorRequest::NewDocument { request_id } => {
                let expected = self.active_document_identity()?;
                let prepared = self.runtime.document_files().prepare_new(&request_id)?;
                let receipt = document_service::commit_prepared_document(
                    self.runtime.document_lifecycle(),
                    &prepared.token,
                    expected.map(|identity| identity.0),
                    expected.map(|identity| identity.1),
                    &request_id,
                )?;
                document_service::mark_current_document_save_required(
                    self.runtime.document_lifecycle(),
                    receipt.document_id,
                    receipt.revision,
                )?;
                self.document_reply(receipt.document_id, receipt.revision, 0)
            }
            EditorRequest::OpenDocument {
                request_id,
                file_name,
                bytes,
            } => self.open_document(request_id, file_name, bytes, false),
            EditorRequest::OpenRecoveryDocument {
                request_id,
                file_name,
                bytes,
            } => self.open_document(request_id, file_name, bytes, true),
            EditorRequest::ActiveDocument => {
                let document = document_query_service::active_document_response(
                    self.runtime.document_queries(),
                )?
                .map(crate::protocol_projection::open_document_response)
                .transpose()?;
                Ok(EditorReply::Document {
                    value: serde_json::to_value(document).map_err(serialization_error)?,
                })
            }
            EditorRequest::Region {
                document_id,
                base_revision,
                sheet_index,
                row_start,
                row_end,
                col_start,
                col_end,
            } => {
                let snapshot = document_query_service::sheet_region_projection_for_command(
                    self.runtime.document_queries(),
                    document_id,
                    base_revision,
                    DocumentRegion {
                        sheet_index,
                        row_start,
                        row_end,
                        col_start,
                        col_end,
                    },
                )?;
                let response = crate::protocol_projection::sheet_region_response(
                    snapshot,
                    crate::resource_limits::MAX_SHEET_REGION_RESPONSE_BYTES,
                )?;
                Ok(EditorReply::Region {
                    value: serde_json::to_value(response).map_err(serialization_error)?,
                })
            }
            EditorRequest::SetCell {
                request_id,
                document_id,
                base_revision,
                sheet_index,
                row,
                col,
                text,
            } => {
                validate_cell_text(&text)?;
                self.editor_command_reply(
                    document_id,
                    base_revision,
                    &request_id,
                    EditorCommand::SetCell {
                        sheet_index,
                        row,
                        col,
                        text,
                    },
                )
            }
            EditorRequest::SetCells {
                request_id,
                document_id,
                base_revision,
                changes,
            } => {
                validate_cell_batch(&changes)?;
                self.editor_command_reply(
                    document_id,
                    base_revision,
                    &request_id,
                    EditorCommand::SetCells {
                        changes: changes
                            .into_iter()
                            .map(|change| crate::domain::CellEditInput {
                                sheet_index: change.sheet_index,
                                row: change.row,
                                col: change.col,
                                text: change.text,
                            })
                            .collect(),
                    },
                )
            }
            EditorRequest::AddRow {
                request_id,
                document_id,
                base_revision,
                sheet_index,
                row_index,
            } => self.editor_command_reply(
                document_id,
                base_revision,
                &request_id,
                EditorCommand::AddRow {
                    sheet_index,
                    row_index,
                },
            ),
            EditorRequest::DeleteRow {
                request_id,
                document_id,
                base_revision,
                sheet_index,
                row_index,
            } => self.editor_command_reply(
                document_id,
                base_revision,
                &request_id,
                EditorCommand::DeleteRow {
                    sheet_index,
                    row_index,
                },
            ),
            EditorRequest::AddColumn {
                request_id,
                document_id,
                base_revision,
                sheet_index,
                col_index,
            } => self.editor_command_reply(
                document_id,
                base_revision,
                &request_id,
                EditorCommand::AddColumn {
                    sheet_index,
                    col_index,
                },
            ),
            EditorRequest::DeleteColumn {
                request_id,
                document_id,
                base_revision,
                sheet_index,
                col_index,
            } => self.editor_command_reply(
                document_id,
                base_revision,
                &request_id,
                EditorCommand::DeleteColumn {
                    sheet_index,
                    col_index,
                },
            ),
            EditorRequest::SetColumnWidth {
                request_id,
                document_id,
                base_revision,
                sheet_index,
                col_index,
                width,
            } => self.editor_command_reply(
                document_id,
                base_revision,
                &request_id,
                EditorCommand::SetColumnWidth {
                    sheet_index,
                    col_index,
                    width,
                },
            ),
            EditorRequest::SetRowHeight {
                request_id,
                document_id,
                base_revision,
                sheet_index,
                row_index,
                height,
            } => self.editor_command_reply(
                document_id,
                base_revision,
                &request_id,
                EditorCommand::SetRowHeight {
                    sheet_index,
                    row_index,
                    height,
                },
            ),
            EditorRequest::AddSheet {
                request_id,
                document_id,
                base_revision,
            } => self.editor_command_reply(
                document_id,
                base_revision,
                &request_id,
                EditorCommand::AddSheet { name: None },
            ),
            EditorRequest::DeleteSheet {
                request_id,
                document_id,
                base_revision,
                sheet_index,
            } => self.editor_command_reply(
                document_id,
                base_revision,
                &request_id,
                EditorCommand::DeleteSheet { sheet_index },
            ),
            EditorRequest::InsertImage {
                request_id,
                document_id,
                base_revision,
                sheet_index,
                row,
                col,
                file_name,
                bytes,
            } => {
                let token = self.runtime.images().stage(file_name, bytes)?;
                let staged = self.runtime.images().get(&token)?;
                let command =
                    editor_command_service::insert_image_command(sheet_index, row, col, staged);
                let result = editor_command_service::execute(
                    self.runtime.editor_commands(),
                    document_id,
                    base_revision,
                    &request_id,
                    command,
                );
                self.runtime.images().discard(&token)?;
                self.mutation_reply(result?)
            }
            EditorRequest::SheetImages {
                document_id,
                base_revision,
                sheet_index,
                offset,
                limit,
            } => {
                let (items, next_offset) = document_query_service::sheet_images_for_command(
                    self.runtime.document_queries(),
                    document_id,
                    base_revision,
                    sheet_index,
                    offset,
                    limit,
                )?;
                Ok(EditorReply::Images {
                    items: items.into_iter().map(image_dto).collect(),
                    next_offset,
                })
            }
            EditorRequest::ImageBytes {
                document_id,
                base_revision,
                sheet_index,
                image_id,
            } => Ok(EditorReply::Bytes {
                bytes: document_query_service::image_bytes_for_command(
                    self.runtime.document_queries(),
                    document_id,
                    base_revision,
                    sheet_index,
                    &image_id,
                )?
                .as_ref()
                .to_vec(),
            }),
            EditorRequest::UpdateImage {
                request_id,
                document_id,
                base_revision,
                sheet_index,
                image_id,
                anchor,
            } => self.editor_command_reply(
                document_id,
                base_revision,
                &request_id,
                EditorCommand::UpdateImage {
                    sheet_index,
                    image_id,
                    anchor: domain_image_anchor(anchor),
                },
            ),
            EditorRequest::DeleteImage {
                request_id,
                document_id,
                base_revision,
                sheet_index,
                image_id,
            } => self.editor_command_reply(
                document_id,
                base_revision,
                &request_id,
                EditorCommand::DeleteImage {
                    sheet_index,
                    image_id,
                },
            ),
            EditorRequest::Undo {
                request_id,
                document_id,
                base_revision,
            } => self.mutation_reply(editor_command_service::undo(
                self.runtime.editor_commands(),
                document_id,
                base_revision,
                &request_id,
            )?),
            EditorRequest::Redo {
                request_id,
                document_id,
                base_revision,
            } => self.mutation_reply(editor_command_service::redo(
                self.runtime.editor_commands(),
                document_id,
                base_revision,
                &request_id,
            )?),
            EditorRequest::Search {
                document_id,
                base_revision,
                query,
                current_sheet_index,
                all_sheets,
            } => {
                let outcome = self.runtime.search_queries().search(
                    document_id,
                    base_revision,
                    &query,
                    if all_sheets {
                        SearchScope::AllSheets
                    } else {
                        SearchScope::CurrentSheet
                    },
                    current_sheet_index,
                )?;
                let response = crate::protocol_projection::search_response(outcome)?;
                Ok(EditorReply::Search {
                    value: serde_json::to_value(response).map_err(serialization_error)?,
                })
            }
            EditorRequest::PrepareSave {
                request_id,
                document_id,
                base_revision,
                target_name,
            } => {
                if self.pending_saves.contains_key(&request_id) {
                    return Err(AppError::DocumentStateInvalid(
                        "save token is already in use".to_string(),
                    ));
                }
                let prepared = document_save_service::prepare_current_file_save(
                    self.runtime.document_saves(),
                    document_id,
                    base_revision,
                    &target_name,
                )?;
                let file_name = prepared.output_name.clone();
                let bytes = prepared.bytes.clone();
                self.pending_saves.insert(request_id.clone(), prepared);
                Ok(EditorReply::SavePrepared {
                    save_token: request_id,
                    file_name,
                    bytes,
                })
            }
            EditorRequest::PrepareExport {
                document_id,
                base_revision,
                target_name,
            } => {
                let prepared = document_save_service::prepare_current_file_export(
                    self.runtime.document_saves(),
                    document_id,
                    base_revision,
                    &target_name,
                )?;
                Ok(EditorReply::ExportPrepared {
                    file_name: prepared.output_name,
                    bytes: prepared.bytes,
                })
            }
            EditorRequest::SaveLocal { .. }
            | EditorRequest::CheckpointRecovery { .. }
            | EditorRequest::ClearRecovery
            | EditorRequest::ListLocalDocuments
            | EditorRequest::OpenLocalDocument { .. }
            | EditorRequest::DeleteLocalDocument { .. } => Err(AppError::Internal(
                "browser persistence requests must be handled by the Web worker".to_string(),
            )),
            EditorRequest::CommitSave { save_token, path } => {
                let prepared = self.pending_saves.remove(&save_token).ok_or_else(|| {
                    AppError::DocumentStateInvalid("save token is missing or expired".to_string())
                })?;
                let response = document_save_service::commit_current_file_save_projected(
                    self.runtime.document_saves(),
                    path,
                    prepared,
                    || Ok(()),
                    crate::protocol_projection::saved_document_response,
                )?;
                Ok(EditorReply::Saved {
                    value: serde_json::to_value(response).map_err(serialization_error)?,
                })
            }
            EditorRequest::AbortSave { save_token } => {
                self.pending_saves.remove(&save_token);
                Ok(EditorReply::Empty)
            }
            EditorRequest::CloseDocument {
                request_id,
                document_id,
                base_revision,
            } => {
                document_service::close_current_document(
                    self.runtime.document_lifecycle(),
                    document_id,
                    base_revision,
                    &request_id,
                )?;
                self.pending_saves.clear();
                Ok(EditorReply::Closed)
            }
        }
    }

    fn document_reply(
        &self,
        document_id: u64,
        revision: u64,
        preferred_sheet_index: usize,
    ) -> Result<EditorReply, AppError> {
        let snapshot = document_query_service::current_document_projection_for_command(
            self.runtime.document_queries(),
            document_id,
            revision,
            preferred_sheet_index,
        )?;
        let response = crate::protocol_projection::open_document_response(snapshot)?;
        Ok(EditorReply::Document {
            value: serde_json::to_value(response).map_err(serialization_error)?,
        })
    }

    fn open_document(
        &mut self,
        request_id: String,
        file_name: String,
        bytes: Vec<u8>,
        recovered: bool,
    ) -> Result<EditorReply, AppError> {
        let expected = self.active_document_identity()?;
        let source_identity = format!("memory:{file_name}:{}", bytes.len());
        let prepared = self.runtime.document_files().prepare_open(
            &request_id,
            &source_identity,
            Box::new(MemoryOpenSource(OpenDocumentSource {
                path: file_name.clone(),
                bytes,
                file_name: Some(file_name),
            })),
        )?;
        let receipt = document_service::commit_prepared_document(
            self.runtime.document_lifecycle(),
            &prepared.token,
            expected.map(|identity| identity.0),
            expected.map(|identity| identity.1),
            &request_id,
        )?;
        if recovered {
            document_service::mark_current_document_save_required(
                self.runtime.document_lifecycle(),
                receipt.document_id,
                receipt.revision,
            )?;
        }
        self.document_reply(receipt.document_id, receipt.revision, 0)
    }

    fn active_document_identity(&self) -> Result<Option<(u64, u64)>, AppError> {
        Ok(
            document_query_service::active_document_response(self.runtime.document_queries())?.map(
                |document| {
                    (
                        document.editor_session.document_id,
                        document.editor_session.revision,
                    )
                },
            ),
        )
    }

    fn mutation_reply(
        &self,
        outcome: crate::projection_model::MutationOutcome,
    ) -> Result<EditorReply, AppError> {
        let response = crate::protocol_projection::mutation_response(outcome);
        Ok(EditorReply::Mutation {
            value: serde_json::to_value(response).map_err(serialization_error)?,
        })
    }

    fn editor_command_reply(
        &self,
        document_id: u64,
        base_revision: u64,
        request_id: &str,
        command: EditorCommand,
    ) -> Result<EditorReply, AppError> {
        let outcome = editor_command_service::execute(
            self.runtime.editor_commands(),
            document_id,
            base_revision,
            request_id,
            command,
        )?;
        self.mutation_reply(outcome)
    }
}

fn image_dto(value: crate::document_data::SheetImage) -> SheetImageDto {
    SheetImageDto {
        id: value.id,
        media_id: value.media_id,
        mime_type: value.mime_type,
        intrinsic_width: value.intrinsic_width,
        intrinsic_height: value.intrinsic_height,
        anchor: match value.anchor {
            crate::document_data::ImageAnchor::OneCell {
                from,
                width_emu,
                height_emu,
            } => ImageAnchorDto::OneCell {
                from: image_marker_dto(from),
                width_emu: u32::try_from(width_emu).unwrap_or(u32::MAX),
                height_emu: u32::try_from(height_emu).unwrap_or(u32::MAX),
            },
            crate::document_data::ImageAnchor::TwoCell { from, to } => ImageAnchorDto::TwoCell {
                from: image_marker_dto(from),
                to: image_marker_dto(to),
            },
        },
        z_index: value.z_index,
        renderable: value.renderable,
    }
}

fn image_marker_dto(value: crate::document_data::ImageMarker) -> ImageMarkerDto {
    ImageMarkerDto {
        row: value.row,
        col: value.col,
        row_offset_emu: value.row_offset_emu,
        col_offset_emu: value.col_offset_emu,
    }
}

fn domain_image_anchor(value: ImageAnchorDto) -> crate::document_data::ImageAnchor {
    match value {
        ImageAnchorDto::OneCell {
            from,
            width_emu,
            height_emu,
        } => crate::document_data::ImageAnchor::OneCell {
            from: domain_image_marker(from),
            width_emu: i64::from(width_emu),
            height_emu: i64::from(height_emu),
        },
        ImageAnchorDto::TwoCell { from, to } => crate::document_data::ImageAnchor::TwoCell {
            from: domain_image_marker(from),
            to: domain_image_marker(to),
        },
    }
}

fn domain_image_marker(value: ImageMarkerDto) -> crate::document_data::ImageMarker {
    crate::document_data::ImageMarker {
        row: value.row,
        col: value.col,
        row_offset_emu: value.row_offset_emu,
        col_offset_emu: value.col_offset_emu,
    }
}

fn serialization_error(error: serde_json::Error) -> AppError {
    AppError::Internal(format!("failed to serialize editor response: {error}"))
}

fn validate_cell_text(text: &str) -> Result<(), AppError> {
    if text.len() <= crate::resource_limits::MAX_CELL_TEXT_BYTES {
        return Ok(());
    }
    Err(AppError::ResourceLimitExceeded(format!(
        "cell text requires {} bytes; the maximum is {} bytes",
        text.len(),
        crate::resource_limits::MAX_CELL_TEXT_BYTES
    )))
}

fn validate_cell_batch(changes: &[CellEdit]) -> Result<(), AppError> {
    if changes.len() > crate::resource_limits::MAX_SET_CELL_CHANGES {
        return Err(AppError::ResourceLimitExceeded(format!(
            "set_cells accepts at most {} changes",
            crate::resource_limits::MAX_SET_CELL_CHANGES
        )));
    }
    let mut bytes = 0usize;
    for change in changes {
        validate_cell_text(&change.text)?;
        bytes = bytes.checked_add(change.text.len()).ok_or_else(|| {
            AppError::ResourceLimitExceeded("set_cells text bytes overflowed".to_string())
        })?;
    }
    if bytes > crate::resource_limits::MAX_MUTATION_TEXT_BYTES {
        return Err(AppError::ResourceLimitExceeded(format!(
            "set_cells text requires {bytes} bytes; the maximum is {} bytes",
            crate::resource_limits::MAX_MUTATION_TEXT_BYTES
        )));
    }
    Ok(())
}

impl From<AppError> for AppErrorDto {
    fn from(value: AppError) -> Self {
        Self {
            code: value.code().to_string(),
            message: value.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn facade_drives_new_edit_save_and_commit_without_platform_io() {
        let mut facade = CoreFacade::default();
        let EditorReply::Document { value } = facade
            .execute(EditorRequest::NewDocument {
                request_id: "new-1".to_string(),
            })
            .expect("new document")
        else {
            panic!("expected document reply")
        };
        let document_id = value["editorSession"]["documentId"]
            .as_str()
            .expect("document id")
            .parse::<u64>()
            .expect("numeric id");
        let revision = value["editorSession"]["revision"]
            .as_str()
            .expect("revision")
            .parse::<u64>()
            .expect("numeric revision");
        assert_eq!(value["editorSession"]["editorState"]["isDirty"], true);

        let EditorReply::Mutation { value } = facade
            .execute(EditorRequest::SetCell {
                request_id: "edit-1".to_string(),
                document_id,
                base_revision: revision,
                sheet_index: 0,
                row: 0,
                col: 0,
                text: "pure Rust".to_string(),
            })
            .expect("edit")
        else {
            panic!("expected mutation reply")
        };
        let revision = value["revision"]
            .as_str()
            .expect("revision")
            .parse::<u64>()
            .expect("numeric revision");

        let EditorReply::SavePrepared {
            save_token,
            file_name,
            bytes,
        } = facade
            .execute(EditorRequest::PrepareSave {
                request_id: "save-1".to_string(),
                document_id,
                base_revision: revision,
                target_name: "book.xlsx".to_string(),
            })
            .expect("prepare save")
        else {
            panic!("expected save reply")
        };
        assert_eq!(file_name, "book.xlsx");
        assert!(!bytes.is_empty());
        let EditorReply::Saved { value } = facade
            .execute(EditorRequest::CommitSave {
                save_token,
                path: "book.xlsx".to_string(),
            })
            .expect("commit save")
        else {
            panic!("expected saved reply")
        };
        assert_eq!(value["editorSession"]["editorState"]["isDirty"], false);
    }

    #[test]
    fn export_can_be_lossy_without_changing_document_state() {
        let mut facade = CoreFacade::default();
        let EditorReply::Document { value } = facade
            .execute(EditorRequest::NewDocument {
                request_id: "export-source".to_string(),
            })
            .expect("new document")
        else {
            panic!("expected document reply")
        };
        let document_id = value["editorSession"]["documentId"]
            .as_str()
            .expect("document id")
            .parse::<u64>()
            .expect("numeric id");
        let revision = value["editorSession"]["revision"]
            .as_str()
            .expect("revision")
            .parse::<u64>()
            .expect("numeric revision");

        let error = facade
            .execute(EditorRequest::PrepareSave {
                request_id: "lossy-native-save".to_string(),
                document_id,
                base_revision: revision,
                target_name: "copy.csv".to_string(),
            })
            .expect_err("native save must reject a lossy target");
        assert_eq!(error.code, "document_state_invalid");

        let EditorReply::ExportPrepared { file_name, bytes } = facade
            .execute(EditorRequest::PrepareExport {
                document_id,
                base_revision: revision,
                target_name: "copy.csv".to_string(),
            })
            .expect("prepare export")
        else {
            panic!("expected export reply")
        };
        assert_eq!(file_name, "copy.csv");
        assert!(!bytes.is_empty());

        let EditorReply::Document { value } = facade
            .execute(EditorRequest::ActiveDocument)
            .expect("active document")
        else {
            panic!("expected document reply")
        };
        assert_eq!(value["document"]["fileName"], "untitled.xlsx");
        assert_eq!(
            value["editorSession"]["revision"],
            serde_json::Value::String(revision.to_string())
        );
        assert_eq!(value["editorSession"]["editorState"]["isDirty"], true);
    }

    #[test]
    fn recovery_open_requires_an_explicit_save() {
        let mut facade = CoreFacade::default();
        let EditorReply::Document { value } = facade
            .execute(EditorRequest::NewDocument {
                request_id: "recovery-source".to_string(),
            })
            .expect("new document")
        else {
            panic!("expected document reply")
        };
        let document_id = value["editorSession"]["documentId"]
            .as_str()
            .expect("document id")
            .parse::<u64>()
            .expect("numeric id");
        let revision = value["editorSession"]["revision"]
            .as_str()
            .expect("revision")
            .parse::<u64>()
            .expect("numeric revision");
        let EditorReply::SavePrepared {
            save_token, bytes, ..
        } = facade
            .execute(EditorRequest::PrepareSave {
                request_id: "recovery-bytes".to_string(),
                document_id,
                base_revision: revision,
                target_name: "recovered.xlsx".to_string(),
            })
            .expect("prepare recovery bytes")
        else {
            panic!("expected prepared save")
        };
        facade
            .execute(EditorRequest::AbortSave { save_token })
            .expect("abort prepared save");

        let EditorReply::Document { value } = facade
            .execute(EditorRequest::OpenRecoveryDocument {
                request_id: "open-recovery".to_string(),
                file_name: "recovered.xlsx".to_string(),
                bytes,
            })
            .expect("open recovery")
        else {
            panic!("expected recovered document")
        };

        assert_eq!(value["editorSession"]["editorState"]["isDirty"], true);
    }
}
