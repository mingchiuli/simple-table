use std::sync::Arc;

use crate::application::file_operation_replay::{
    FileOperationAdmission, FileOperationFingerprint, FileOperationReplayCoordinator,
    cancelled_operation_error, completed_operation_error, pending_operation_error,
};
use crate::application::mutation_replay::{self, MutationReplayCoordinator};
use crate::application::prepared_document_repository::PreparedDocumentRepository;
use crate::application::prepared_source_port::{
    PreparedSourceAdoption, PreparedSourceAdoptionPort,
};
use crate::application::search_ports::SearchIndexMaintenancePort;
use crate::error::AppError;
use crate::projection_model::{FileOperationKind, FileOperationReceipt};
use crate::state::state::ActiveDocumentRepository;

#[derive(Clone)]
pub struct DocumentLifecycleService {
    documents: ActiveDocumentRepository,
    prepared_documents: PreparedDocumentRepository,
    mutation_replays: Arc<MutationReplayCoordinator>,
    search_indexes: Arc<dyn SearchIndexMaintenancePort>,
    prepared_source_adoptions: Arc<dyn PreparedSourceAdoptionPort>,
    file_operations: FileOperationReplayCoordinator,
}

impl DocumentLifecycleService {
    pub(crate) fn new(
        documents: ActiveDocumentRepository,
        prepared_documents: PreparedDocumentRepository,
        mutation_replays: Arc<MutationReplayCoordinator>,
        search_indexes: Arc<dyn SearchIndexMaintenancePort>,
        prepared_source_adoptions: Arc<dyn PreparedSourceAdoptionPort>,
        file_operations: FileOperationReplayCoordinator,
    ) -> Self {
        Self {
            documents,
            prepared_documents,
            mutation_replays,
            search_indexes,
            prepared_source_adoptions,
            file_operations,
        }
    }

    fn documents(&self) -> &ActiveDocumentRepository {
        &self.documents
    }

    fn prepared_documents(&self) -> &PreparedDocumentRepository {
        &self.prepared_documents
    }

    fn mutation_replays(&self) -> &Arc<MutationReplayCoordinator> {
        &self.mutation_replays
    }

    fn search_indexes(&self) -> &dyn SearchIndexMaintenancePort {
        self.search_indexes.as_ref()
    }

    fn begin_prepared_source_adoption(
        &self,
        source_path: Option<&std::path::Path>,
        file_name: &str,
    ) -> Result<Box<dyn PreparedSourceAdoption>, AppError> {
        self.prepared_source_adoptions
            .begin_adoption(source_path, file_name)
    }

    fn file_operations(&self) -> &FileOperationReplayCoordinator {
        &self.file_operations
    }
}

/// Commits a prepared document and retires every service resource owned by the
/// previous document before its state is released.
pub fn commit_prepared_document(
    service: &DocumentLifecycleService,
    token: &str,
    expected_document_id: Option<u64>,
    expected_revision: Option<u64>,
    operation_id: &str,
) -> Result<FileOperationReceipt, AppError> {
    let fingerprint =
        FileOperationFingerprint::open(token, expected_document_id, expected_revision);
    let reservation = match service
        .file_operations()
        .reserve(operation_id, fingerprint)?
    {
        FileOperationAdmission::Execute(reservation) => reservation,
        FileOperationAdmission::Pending => {
            return Err(pending_operation_error(FileOperationKind::Open));
        }
        FileOperationAdmission::Completed => {
            return Err(completed_operation_error(FileOperationKind::Open));
        }
        FileOperationAdmission::Failed(error) => return Err(error),
        FileOperationAdmission::Cancelled => {
            return Err(cancelled_operation_error(FileOperationKind::Open));
        }
    };
    let result = (|| {
        let checkout = service.prepared_documents().checkout(token)?;
        let replacement = service
            .documents()
            .begin_replacement(expected_document_id, expected_revision)?;
        let source_adoption = service.begin_prepared_source_adoption(
            checkout.document().source_path.as_deref(),
            &checkout.document().editor_state.file_data().file_name,
        )?;
        let receipt = FileOperationReceipt {
            kind: FileOperationKind::Open,
            document_id: checkout.document().editor_state.document_id(),
            revision: checkout.document().editor_state.revision(),
            path: checkout.document().editor_state.file_data().path.clone(),
            file_name: checkout
                .document()
                .editor_state
                .file_data()
                .file_name
                .clone(),
        };
        let (prepared, _prepared_commit) = checkout.commit();
        let replacement = replacement.finish(prepared.editor_state)?;
        source_adoption.commit();
        let document_id = replacement.document_id;
        let previous_document = replacement.previous_document;
        let _active_handle = replacement.active_handle;

        if let Some(previous_document_id) = previous_document
            .as_ref()
            .map(|handle| handle.document_id())
            && previous_document_id != document_id
        {
            retire_document_runtime(service, previous_document_id);
        }
        drop(previous_document);
        service
            .search_indexes()
            .rebuild_all_sheets_index(document_id);
        Ok(receipt)
    })();
    match result {
        Ok(receipt) => Ok(reservation.complete(receipt)),
        Err(error) => Err(reservation.fail(error)),
    }
}

pub fn close_current_document(
    service: &DocumentLifecycleService,
    document_id: u64,
    base_revision: u64,
    operation_id: &str,
) -> Result<FileOperationReceipt, AppError> {
    let fingerprint = FileOperationFingerprint::close(document_id, base_revision);
    let reservation = match service
        .file_operations()
        .reserve(operation_id, fingerprint)?
    {
        FileOperationAdmission::Execute(reservation) => reservation,
        FileOperationAdmission::Pending => {
            return Err(pending_operation_error(FileOperationKind::Close));
        }
        FileOperationAdmission::Completed => {
            return Err(completed_operation_error(FileOperationKind::Close));
        }
        FileOperationAdmission::Failed(error) => return Err(error),
        FileOperationAdmission::Cancelled => {
            return Err(cancelled_operation_error(FileOperationKind::Close));
        }
    };
    let result = (|| {
        let handle = service.documents().read_handle(document_id)?;
        let (path, file_name) = {
            let state = handle.read_for_command(document_id, base_revision)?;
            (
                state.file_data().path.clone(),
                state.file_data().file_name.clone(),
            )
        };
        let closed_document = service
            .documents()
            .close_for_command(document_id, base_revision)?;
        if let Some(closed_document_id) =
            closed_document.as_ref().map(|handle| handle.document_id())
        {
            retire_document_runtime(service, closed_document_id);
        }
        drop(closed_document);
        Ok(FileOperationReceipt {
            kind: FileOperationKind::Close,
            document_id,
            revision: base_revision,
            path,
            file_name,
        })
    })();
    match result {
        Ok(receipt) => Ok(reservation.complete(receipt)),
        Err(error) => Err(reservation.fail(error)),
    }
}

fn retire_document_runtime(service: &DocumentLifecycleService, document_id: u64) {
    service.search_indexes().cancel_document_jobs(document_id);
    mutation_replay::retire_document(service.mutation_replays(), document_id);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projection_model::FileOperationLookupStatus;
    use crate::runtime::ApplicationRuntime;

    fn open_new_document(runtime: &ApplicationRuntime) -> FileOperationReceipt {
        let prepared = runtime
            .document_files()
            .prepare_new("prepare-close-test")
            .expect("prepare new");
        commit_prepared_document(
            runtime.document_lifecycle(),
            &prepared.token,
            None,
            None,
            "open-for-close-test",
        )
        .expect("commit prepared")
    }

    #[test]
    fn close_success_is_recoverable_from_the_shared_file_operation_log() {
        let runtime = ApplicationRuntime::default();
        let opened = open_new_document(&runtime);

        let closed = close_current_document(
            runtime.document_lifecycle(),
            opened.document_id,
            opened.revision,
            "close-success",
        )
        .expect("close document");

        assert_eq!(closed.kind, FileOperationKind::Close);
        assert_eq!(closed.document_id, opened.document_id);
        assert_eq!(closed.revision, opened.revision);
        let lookup = runtime
            .document_files()
            .file_operation_result("close-success")
            .expect("lookup");
        assert_eq!(lookup.status, FileOperationLookupStatus::Completed);
        assert_eq!(lookup.receipt, Some(closed));
    }

    #[test]
    fn close_failure_is_terminal_and_keeps_the_document_active() {
        let runtime = ApplicationRuntime::default();
        let opened = open_new_document(&runtime);
        let stale_revision = opened.revision.saturating_add(1);

        assert!(matches!(
            close_current_document(
                runtime.document_lifecycle(),
                opened.document_id,
                stale_revision,
                "close-failed",
            ),
            Err(AppError::DocumentStateInvalid(_))
        ));

        let lookup = runtime
            .document_files()
            .file_operation_result("close-failed")
            .expect("lookup");
        assert_eq!(lookup.status, FileOperationLookupStatus::Failed);
        assert_eq!(
            lookup.error.expect("failure").code,
            "document_state_invalid"
        );
        assert!(
            crate::application::document_query_service::document_capabilities_for_command(
                runtime.document_queries(),
                opened.document_id,
                opened.revision,
            )
            .is_ok()
        );
    }
}
