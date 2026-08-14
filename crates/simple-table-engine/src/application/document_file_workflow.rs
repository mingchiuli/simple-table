use crate::application::document_codec_port::OpenDocumentSource;
use crate::application::document_open_service::{self, DocumentOpenService};
use crate::application::prepared_document_repository::{
    PrepareReservationResult, PreparedDocumentFingerprint,
};
use crate::error::AppError;
use crate::projection_model::PreparedOpenDocument;

pub(crate) trait DocumentOpenSourcePort: Send {
    fn read(self: Box<Self>) -> Result<OpenDocumentSource, AppError>;
}

#[derive(Clone)]
pub struct DocumentFileWorkflowService {
    opens: DocumentOpenService,
}

impl DocumentFileWorkflowService {
    pub(crate) fn new(opens: DocumentOpenService) -> Self {
        Self { opens }
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

    pub(crate) fn prepare_new(
        &self,
        preparation_id: &str,
    ) -> Result<PreparedOpenDocument, AppError> {
        document_open_service::prepare_new_file(&self.opens, preparation_id)
    }

    #[cfg(test)]
    pub(crate) fn is_isolated_from(&self, other: &Self) -> bool {
        self.opens.is_isolated_from(&other.opens)
    }
}
