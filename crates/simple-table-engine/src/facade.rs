use std::collections::HashMap;

use crate::protocol::{
    AppErrorDto, CellEdit, EditorCommand as ProtocolEditorCommand, EditorOutput, EditorReply,
    EditorRequest, EditorResponse, FilterOperatorDto, ImageAnchorDto, ImageMarkerDto,
    SheetImageDto, SortDirectionDto,
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
    pub fn execute(&mut self, command: impl Into<ProtocolEditorCommand>) -> EditorResponse {
        let ProtocolEditorCommand {
            request,
            attachment,
        } = command.into();
        validate_request_attachment(&request, &attachment)?;
        let mut output_attachment = None;
        let reply = self
            .execute_inner(request, attachment, &mut output_attachment)
            .map_err(AppErrorDto::from)?;
        Ok(EditorOutput {
            reply,
            attachment: output_attachment,
        })
    }

    fn execute_inner(
        &mut self,
        request: EditorRequest,
        attachment: Option<Vec<u8>>,
        output_attachment: &mut Option<Vec<u8>>,
    ) -> Result<EditorReply, AppError> {
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
            } => self.open_document(
                request_id,
                file_name,
                attachment.expect("validated open attachment"),
                false,
            ),
            EditorRequest::OpenRecoveryDocument {
                request_id,
                file_name,
            } => self.open_document(
                request_id,
                file_name,
                attachment.expect("validated recovery attachment"),
                true,
            ),
            EditorRequest::ActiveDocument => {
                let document = document_query_service::active_document_response(
                    self.runtime.document_queries(),
                )?
                .map(crate::protocol_projection::open_document_response)
                .transpose()?;
                Ok(EditorReply::Document { value: document })
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
                Ok(EditorReply::Region { value: response })
            }
            EditorRequest::RowsRegion {
                document_id,
                base_revision,
                sheet_index,
                rows,
                col_start,
                col_end,
            } => {
                let snapshots = document_query_service::sheet_rows_region_projection_for_command(
                    self.runtime.document_queries(),
                    document_id,
                    base_revision,
                    sheet_index,
                    &rows,
                    col_start,
                    col_end,
                )?;
                let response = crate::protocol_projection::sheet_rows_region_response(
                    snapshots,
                    crate::resource_limits::MAX_SHEET_REGION_RESPONSE_BYTES,
                )?;
                Ok(EditorReply::RowsRegion { value: response })
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
            EditorRequest::SortRows {
                request_id,
                document_id,
                base_revision,
                sheet_index,
                anchor_row,
                anchor_col,
                direction,
            } => self.editor_command_reply(
                document_id,
                base_revision,
                &request_id,
                EditorCommand::SortRows {
                    sheet_index,
                    anchor_row,
                    anchor_col,
                    direction: match direction {
                        SortDirectionDto::Ascending => crate::domain::SortDirection::Ascending,
                        SortDirectionDto::Descending => crate::domain::SortDirection::Descending,
                    },
                },
            ),
            EditorRequest::SetFilter {
                request_id,
                document_id,
                base_revision,
                sheet_index,
                anchor_row,
                col,
                operator,
                value,
            } => self.editor_command_reply(
                document_id,
                base_revision,
                &request_id,
                EditorCommand::SetFilter {
                    sheet_index,
                    anchor_row,
                    col,
                    operator: match operator {
                        FilterOperatorDto::Equals => crate::domain::FilterOperator::Equals,
                        FilterOperatorDto::NotEquals => crate::domain::FilterOperator::NotEquals,
                        FilterOperatorDto::Contains => crate::domain::FilterOperator::Contains,
                        FilterOperatorDto::Blank => crate::domain::FilterOperator::Blank,
                        FilterOperatorDto::NotBlank => crate::domain::FilterOperator::NotBlank,
                    },
                    value,
                },
            ),
            EditorRequest::ClearFilter {
                request_id,
                document_id,
                base_revision,
                sheet_index,
                col,
            } => self.editor_command_reply(
                document_id,
                base_revision,
                &request_id,
                EditorCommand::ClearFilter { sheet_index, col },
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
            } => {
                let bytes = attachment.expect("validated image attachment");
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
            } => {
                *output_attachment = Some(
                    document_query_service::image_bytes_for_command(
                        self.runtime.document_queries(),
                        document_id,
                        base_revision,
                        sheet_index,
                        &image_id,
                    )?
                    .as_ref()
                    .to_vec(),
                );
                Ok(EditorReply::Bytes)
            }
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
                Ok(EditorReply::Search { value: response })
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
                *output_attachment = Some(bytes);
                Ok(EditorReply::SavePrepared {
                    save_token: request_id,
                    file_name,
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
                *output_attachment = Some(prepared.bytes);
                Ok(EditorReply::ExportPrepared {
                    file_name: prepared.output_name,
                })
            }
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
                Ok(EditorReply::Saved { value: response })
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
            value: Some(response),
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
        outcome: std::sync::Arc<crate::projection_model::MutationOutcome>,
    ) -> Result<EditorReply, AppError> {
        let response = crate::protocol_projection::mutation_response(&outcome);
        Ok(EditorReply::Mutation { value: response })
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

fn validate_request_attachment(
    request: &EditorRequest,
    attachment: &Option<Vec<u8>>,
) -> Result<(), AppErrorDto> {
    let required = matches!(
        request,
        EditorRequest::OpenDocument { .. }
            | EditorRequest::OpenRecoveryDocument { .. }
            | EditorRequest::InsertImage { .. }
    );
    match (required, attachment.is_some()) {
        (true, false) => Err(AppErrorDto {
            code: "invalid_request".to_string(),
            message: "editor request requires a binary attachment".to_string(),
        }),
        (false, true) => Err(AppErrorDto {
            code: "invalid_request".to_string(),
            message: "editor request does not accept a binary attachment".to_string(),
        }),
        _ => Ok(()),
    }
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
        let output = facade
            .execute(EditorRequest::NewDocument {
                request_id: "new-1".to_string(),
            })
            .expect("new document");
        let EditorReply::Document { value: Some(value) } = output.reply else {
            panic!("expected document reply")
        };
        let document_id = value.editor_session.document_id;
        let revision = value.editor_session.revision;
        assert!(value.editor_session.editor_state.is_dirty);

        let output = facade
            .execute(EditorRequest::SetCell {
                request_id: "edit-1".to_string(),
                document_id,
                base_revision: revision,
                sheet_index: 0,
                row: 0,
                col: 0,
                text: "pure Rust".to_string(),
            })
            .expect("edit");
        let EditorReply::Mutation { value } = output.reply else {
            panic!("expected mutation reply")
        };
        let revision = value.revision;

        let output = facade
            .execute(EditorRequest::PrepareSave {
                request_id: "save-1".to_string(),
                document_id,
                base_revision: revision,
                target_name: "book.xlsx".to_string(),
            })
            .expect("prepare save");
        let EditorReply::SavePrepared {
            save_token,
            file_name,
        } = output.reply
        else {
            panic!("expected save reply")
        };
        assert_eq!(file_name, "book.xlsx");
        assert!(output.attachment.is_some_and(|bytes| !bytes.is_empty()));
        let output = facade
            .execute(EditorRequest::CommitSave {
                save_token,
                path: "book.xlsx".to_string(),
            })
            .expect("commit save");
        let EditorReply::Saved { value } = output.reply else {
            panic!("expected saved reply")
        };
        assert!(!value.editor_session.editor_state.is_dirty);
    }

    #[test]
    fn export_can_be_lossy_without_changing_document_state() {
        let mut facade = CoreFacade::default();
        let output = facade
            .execute(EditorRequest::NewDocument {
                request_id: "export-source".to_string(),
            })
            .expect("new document");
        let EditorReply::Document { value: Some(value) } = output.reply else {
            panic!("expected document reply")
        };
        let document_id = value.editor_session.document_id;
        let revision = value.editor_session.revision;

        let error = facade
            .execute(EditorRequest::PrepareSave {
                request_id: "lossy-native-save".to_string(),
                document_id,
                base_revision: revision,
                target_name: "copy.csv".to_string(),
            })
            .expect_err("native save must reject a lossy target");
        assert_eq!(error.code, "document_state_invalid");

        let output = facade
            .execute(EditorRequest::PrepareExport {
                document_id,
                base_revision: revision,
                target_name: "copy.csv".to_string(),
            })
            .expect("prepare export");
        let EditorReply::ExportPrepared { file_name } = output.reply else {
            panic!("expected export reply")
        };
        assert_eq!(file_name, "copy.csv");
        assert!(output.attachment.is_some_and(|bytes| !bytes.is_empty()));

        let output = facade
            .execute(EditorRequest::ActiveDocument)
            .expect("active document");
        let EditorReply::Document { value: Some(value) } = output.reply else {
            panic!("expected document reply")
        };
        assert_eq!(value.document.file_name, "untitled.xlsx");
        assert_eq!(value.editor_session.revision, revision);
        assert!(value.editor_session.editor_state.is_dirty);
    }

    #[test]
    fn recovery_open_requires_an_explicit_save() {
        let mut facade = CoreFacade::default();
        let output = facade
            .execute(EditorRequest::NewDocument {
                request_id: "recovery-source".to_string(),
            })
            .expect("new document");
        let EditorReply::Document { value: Some(value) } = output.reply else {
            panic!("expected document reply")
        };
        let document_id = value.editor_session.document_id;
        let revision = value.editor_session.revision;
        let output = facade
            .execute(EditorRequest::PrepareSave {
                request_id: "recovery-bytes".to_string(),
                document_id,
                base_revision: revision,
                target_name: "recovered.xlsx".to_string(),
            })
            .expect("prepare recovery bytes");
        let EditorReply::SavePrepared { save_token, .. } = output.reply else {
            panic!("expected prepared save")
        };
        let bytes = output.attachment.expect("recovery bytes");
        facade
            .execute(EditorRequest::AbortSave { save_token })
            .expect("abort prepared save");

        let output = facade
            .execute(ProtocolEditorCommand::with_attachment(
                EditorRequest::OpenRecoveryDocument {
                    request_id: "open-recovery".to_string(),
                    file_name: "recovered.xlsx".to_string(),
                },
                bytes,
            ))
            .expect("open recovery");
        let EditorReply::Document { value: Some(value) } = output.reply else {
            panic!("expected recovered document")
        };

        assert!(value.editor_session.editor_state.is_dirty);
    }
}
