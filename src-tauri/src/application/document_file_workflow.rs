use crate::application::document_codec_port::OpenDocumentSource;
use crate::application::document_open_service::{self, DocumentOpenService};
use crate::application::document_save_service::{self, DocumentSaveService};
use crate::error::AppError;
use crate::projection_model::{PreparedOpenDocument, SavedDocumentOutcome};

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
}

impl DocumentFileWorkflowService {
    pub(crate) fn new(opens: DocumentOpenService, saves: DocumentSaveService) -> Self {
        Self { opens, saves }
    }

    pub(crate) fn prepare_open(
        &self,
        source: Box<dyn DocumentOpenSourcePort>,
    ) -> Result<PreparedOpenDocument, AppError> {
        document_open_service::prepare_open_input(&self.opens, source.read()?)
    }

    pub(crate) fn prepare_new(&self) -> Result<PreparedOpenDocument, AppError> {
        document_open_service::prepare_new_file(&self.opens)
    }

    pub(crate) fn save(
        &self,
        target: Box<dyn DocumentSaveTargetPort>,
        document_id: u64,
        base_revision: u64,
    ) -> Result<SavedDocumentOutcome, AppError> {
        let target_path = target.target_path().to_string();
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
        document_save_service::commit_current_file_save(
            &self.saves,
            target_path,
            prepared,
            move || staged.commit(),
        )
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
        self.opens.is_isolated_from(&other.opens) && self.saves.is_isolated_from(&other.saves)
    }
}
