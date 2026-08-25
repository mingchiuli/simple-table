pub mod content_hash;
pub mod dirty_tracker;
pub mod editor_session;
pub mod editor_state;
pub mod history_store;
pub(crate) mod search_document;
pub(crate) mod table_filter;

mod repository;

pub(crate) use repository::{ActiveDocumentRepository, DocumentHandle};
