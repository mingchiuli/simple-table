use crate::application::document_codec_port::OpenDocumentSource;
use crate::application::document_open_service::{self, DocumentOpenService};
use crate::application::document_save_service::{self, DocumentSaveService};
use crate::application::file_operation_replay::{
    FileOperationAdmission, FileOperationFingerprint, FileOperationReplayCoordinator,
    completed_operation_error, pending_operation_error,
};
use crate::error::AppError;
use crate::projection_model::{
    FileOperationKind, FileOperationLookup, FileOperationReceipt, PreparedOpenDocument,
    SavedDocumentOutcome,
};

pub(crate) trait DocumentOpenSourcePort: Send {
    fn read(self: Box<Self>) -> Result<OpenDocumentSource, AppError>;
}

pub(crate) trait StagedDocumentWrite: Send {
    fn commit(self: Box<Self>) -> Result<(), AppError>;
}

pub(crate) trait DocumentSaveTargetPort: Send {
    fn target_path(&self) -> &str;
    fn ensure_authorized(&self, current_document_path: &str) -> Result<(), AppError>;
    fn stage(
        self: Box<Self>,
        bytes: &[u8],
        output_name: &str,
    ) -> Result<Box<dyn StagedDocumentWrite>, AppError>;
}

pub(crate) trait DocumentExportTargetPort: Send {
    fn target_path_or_name(&self) -> &str;
    fn write(self: Box<Self>, bytes: &[u8]) -> Result<String, AppError>;
}

#[derive(Clone)]
pub struct DocumentFileWorkflowService {
    opens: DocumentOpenService,
    saves: DocumentSaveService,
    file_operations: FileOperationReplayCoordinator,
}

impl DocumentFileWorkflowService {
    pub(crate) fn new(
        opens: DocumentOpenService,
        saves: DocumentSaveService,
        file_operations: FileOperationReplayCoordinator,
    ) -> Self {
        Self {
            opens,
            saves,
            file_operations,
        }
    }

    pub(crate) fn prepare_open(
        &self,
        source: Box<dyn DocumentOpenSourcePort>,
    ) -> Result<PreparedOpenDocument, AppError> {
        document_open_service::prepare_open_input(&self.opens, source.read()?)
    }

    pub(crate) fn prepare_open_projected<T>(
        &self,
        source: Box<dyn DocumentOpenSourcePort>,
        project: impl FnOnce(PreparedOpenDocument) -> Result<T, AppError>,
    ) -> Result<T, AppError> {
        self.project_prepared(self.prepare_open(source)?, project)
    }

    pub(crate) fn prepare_new(&self) -> Result<PreparedOpenDocument, AppError> {
        document_open_service::prepare_new_file(&self.opens)
    }

    pub(crate) fn prepare_new_projected<T>(
        &self,
        project: impl FnOnce(PreparedOpenDocument) -> Result<T, AppError>,
    ) -> Result<T, AppError> {
        self.project_prepared(self.prepare_new()?, project)
    }

    pub(crate) fn save_projected<T>(
        &self,
        target: Box<dyn DocumentSaveTargetPort>,
        document_id: u64,
        base_revision: u64,
        operation_id: &str,
        project: impl FnOnce(SavedDocumentOutcome) -> Result<T, AppError>,
    ) -> Result<T, AppError> {
        let target_path = target.target_path().to_string();
        let fingerprint = FileOperationFingerprint::save(&target_path, document_id, base_revision);
        let reservation = match self.file_operations.reserve(operation_id, fingerprint)? {
            FileOperationAdmission::Execute(reservation) => reservation,
            FileOperationAdmission::Pending => {
                return Err(pending_operation_error(FileOperationKind::Save));
            }
            FileOperationAdmission::Completed => {
                return Err(completed_operation_error(FileOperationKind::Save));
            }
            FileOperationAdmission::Failed(error) => return Err(error),
        };
        let result = (|| {
            let current_path = document_save_service::current_document_path_for_command(
                &self.saves,
                document_id,
                base_revision,
            )?;
            target.ensure_authorized(&current_path)?;
            let prepared = document_save_service::prepare_current_file_save(
                &self.saves,
                document_id,
                base_revision,
                &target_path,
            )?;
            let staged = match target.stage(&prepared.bytes, &prepared.output_name) {
                Ok(staged) => staged,
                Err(error) => {
                    document_save_service::abort_prepared_file_save(prepared);
                    return Err(error);
                }
            };
            document_save_service::commit_current_file_save_projected(
                &self.saves,
                target_path,
                prepared,
                move || staged.commit(),
                |outcome| {
                    let receipt = file_operation_receipt(FileOperationKind::Save, &outcome)?;
                    project(outcome).map(|projected| (projected, receipt))
                },
            )
        })();
        match result {
            Ok((projected, receipt)) => {
                reservation.complete(receipt);
                Ok(projected)
            }
            Err(error) => Err(reservation.fail(error)),
        }
    }

    pub(crate) fn file_operation_result(
        &self,
        operation_id: &str,
    ) -> Result<FileOperationLookup, AppError> {
        self.file_operations.get(operation_id)
    }

    fn project_prepared<T>(
        &self,
        prepared: PreparedOpenDocument,
        project: impl FnOnce(PreparedOpenDocument) -> Result<T, AppError>,
    ) -> Result<T, AppError> {
        let token = prepared.token.clone();
        match project(prepared) {
            Ok(projected) => Ok(projected),
            Err(error) => {
                if let Err(cleanup_error) =
                    document_open_service::abort_prepared_document(&self.opens, &token)
                {
                    return Err(AppError::Internal(format!(
                        "prepared document projection failed ({error}) and cleanup failed ({cleanup_error})"
                    )));
                }
                Err(error)
            }
        }
    }

    pub(crate) fn export(
        &self,
        target: Box<dyn DocumentExportTargetPort>,
        document_id: u64,
        base_revision: u64,
    ) -> Result<String, AppError> {
        let prepared = document_save_service::prepare_current_file_export(
            &self.saves,
            document_id,
            base_revision,
            target.target_path_or_name(),
        )?;
        target.write(&prepared.bytes)
    }

    #[cfg(test)]
    pub(crate) fn is_isolated_from(&self, other: &Self) -> bool {
        self.opens.is_isolated_from(&other.opens)
            && self.saves.is_isolated_from(&other.saves)
            && !self
                .file_operations
                .is_same_instance(&other.file_operations)
    }
}

fn file_operation_receipt(
    kind: FileOperationKind,
    outcome: &SavedDocumentOutcome,
) -> Result<FileOperationReceipt, AppError> {
    let (path, file_name) = if let Some(document) = &outcome.document {
        (document.path.clone(), document.file_name.clone())
    } else if let Some(identity) = &outcome.identity {
        (identity.path.clone(), identity.file_name.clone())
    } else {
        return Err(AppError::Internal(
            "saved outcome contains neither a document manifest nor an identity".to_string(),
        ));
    };
    Ok(FileOperationReceipt {
        kind,
        document_id: outcome.editor_session.document_id,
        revision: outcome.editor_session.revision,
        path,
        file_name,
    })
}
