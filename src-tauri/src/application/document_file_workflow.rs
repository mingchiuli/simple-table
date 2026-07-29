use crate::application::document_codec_port::OpenDocumentSource;
use crate::application::document_open_service::{self, DocumentOpenService};
use crate::application::document_save_service::{self, DocumentSaveService};
use crate::application::file_operation_replay::{
    FileOperationAdmission, FileOperationFingerprint, FileOperationReplayCoordinator,
    cancelled_operation_error, completed_operation_error, pending_operation_error,
};
use crate::application::prepared_document_repository::{
    PrepareReservationResult, PreparedDocumentFingerprint,
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
        preparation_id: &str,
        source_identity: &str,
        source: Box<dyn DocumentOpenSourcePort>,
    ) -> Result<PreparedOpenDocument, AppError> {
        let fingerprint = PreparedDocumentFingerprint::open(source_identity);
        match self
            .opens
            .prepared_documents()
            .reserve(preparation_id, fingerprint)?
        {
            PrepareReservationResult::Execute(reservation) => {
                document_open_service::prepare_open_input(&self.opens, source.read()?, reservation)
            }
            PrepareReservationResult::Replay => document_open_service::replay_prepared_document(
                &self.opens,
                preparation_id,
                fingerprint,
            ),
        }
    }

    pub(crate) fn prepare_open_projected<T>(
        &self,
        preparation_id: &str,
        source_identity: &str,
        source: Box<dyn DocumentOpenSourcePort>,
        project: impl FnOnce(PreparedOpenDocument) -> Result<T, AppError>,
    ) -> Result<T, AppError> {
        self.project_prepared(
            self.prepare_open(preparation_id, source_identity, source)?,
            project,
        )
    }

    pub(crate) fn prepare_new(
        &self,
        preparation_id: &str,
    ) -> Result<PreparedOpenDocument, AppError> {
        document_open_service::prepare_new_file(&self.opens, preparation_id)
    }

    pub(crate) fn prepare_new_projected<T>(
        &self,
        preparation_id: &str,
        project: impl FnOnce(PreparedOpenDocument) -> Result<T, AppError>,
    ) -> Result<T, AppError> {
        self.project_prepared(self.prepare_new(preparation_id)?, project)
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
            FileOperationAdmission::Cancelled => {
                return Err(cancelled_operation_error(FileOperationKind::Save));
            }
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
        operation_id: &str,
        default_name: &str,
        document_id: u64,
        base_revision: u64,
        pick_target: impl FnOnce() -> Result<Option<Box<dyn DocumentExportTargetPort>>, AppError>,
    ) -> Result<Option<FileOperationReceipt>, AppError> {
        let fingerprint =
            FileOperationFingerprint::export(default_name, document_id, base_revision);
        let reservation = match self.file_operations.reserve(operation_id, fingerprint)? {
            FileOperationAdmission::Execute(reservation) => reservation,
            FileOperationAdmission::Pending => {
                return Err(pending_operation_error(FileOperationKind::Export));
            }
            FileOperationAdmission::Completed => {
                return Err(completed_operation_error(FileOperationKind::Export));
            }
            FileOperationAdmission::Failed(error) => return Err(error),
            FileOperationAdmission::Cancelled => return Ok(None),
        };
        let result = (|| {
            let Some(target) = pick_target()? else {
                return Ok(None);
            };
            let output_name = target.target_path_or_name().to_string();
            let prepared = document_save_service::prepare_current_file_export(
                &self.saves,
                document_id,
                base_revision,
                &output_name,
            )?;
            let path = target.write(&prepared.bytes)?;
            Ok(Some(FileOperationReceipt {
                kind: FileOperationKind::Export,
                document_id,
                revision: base_revision,
                path,
                file_name: output_name,
            }))
        })();
        match result {
            Ok(Some(receipt)) => Ok(Some(reservation.complete(receipt))),
            Ok(None) => {
                reservation.cancel();
                Ok(None)
            }
            Err(error) => Err(reservation.fail(error)),
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::document_service;
    use crate::runtime::ApplicationRuntime;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct TestExportTarget {
        writes: Arc<AtomicUsize>,
    }

    impl DocumentExportTargetPort for TestExportTarget {
        fn target_path_or_name(&self) -> &str {
            "export.xlsx"
        }

        fn write(self: Box<Self>, _bytes: &[u8]) -> Result<String, AppError> {
            self.writes.fetch_add(1, Ordering::SeqCst);
            Ok("/tmp/export.xlsx".to_string())
        }
    }

    fn active_document(runtime: &ApplicationRuntime) -> FileOperationReceipt {
        let prepared = runtime
            .document_files()
            .prepare_new("prepare-export-test")
            .expect("prepare document");
        document_service::commit_prepared_document(
            runtime.document_lifecycle(),
            &prepared.token,
            None,
            None,
            "open-export-test",
        )
        .expect("open document")
    }

    #[test]
    fn completed_export_retry_does_not_reopen_picker_or_rewrite_file() {
        let runtime = ApplicationRuntime::default();
        let active = active_document(&runtime);
        let picks = Arc::new(AtomicUsize::new(0));
        let writes = Arc::new(AtomicUsize::new(0));
        let first_picks = Arc::clone(&picks);
        let first_writes = Arc::clone(&writes);

        let receipt = runtime
            .document_files()
            .export(
                "export-operation",
                "export.xlsx",
                active.document_id,
                active.revision,
                move || {
                    first_picks.fetch_add(1, Ordering::SeqCst);
                    Ok(Some(Box::new(TestExportTarget {
                        writes: first_writes,
                    })))
                },
            )
            .expect("export succeeds")
            .expect("export receipt");
        assert_eq!(receipt.kind, FileOperationKind::Export);

        let retry = runtime.document_files().export(
            "export-operation",
            "export.xlsx",
            active.document_id,
            active.revision,
            || panic!("completed retry must not reopen picker"),
        );

        assert!(matches!(retry, Err(AppError::DocumentStateInvalid(_))));
        assert_eq!(picks.load(Ordering::SeqCst), 1);
        assert_eq!(writes.load(Ordering::SeqCst), 1);
        assert_eq!(
            runtime
                .document_files()
                .file_operation_result("export-operation")
                .expect("lookup")
                .status,
            crate::projection_model::FileOperationLookupStatus::Completed
        );
    }

    #[test]
    fn cancelled_export_retry_does_not_reopen_picker() {
        let runtime = ApplicationRuntime::default();
        let active = active_document(&runtime);

        assert!(
            runtime
                .document_files()
                .export(
                    "cancel-export-operation",
                    "export.xlsx",
                    active.document_id,
                    active.revision,
                    || Ok(None),
                )
                .expect("cancel succeeds")
                .is_none()
        );
        assert!(
            runtime
                .document_files()
                .export(
                    "cancel-export-operation",
                    "export.xlsx",
                    active.document_id,
                    active.revision,
                    || panic!("cancelled retry must not reopen picker"),
                )
                .expect("cancel replay succeeds")
                .is_none()
        );
    }
}
