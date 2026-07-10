pub mod codec {
    pub mod reader;
    pub mod writer;
}

pub mod atomic_file;
pub mod document;
pub mod document_body;
pub mod document_memento;
pub mod document_memento_budget;
pub mod document_model;
pub mod document_patches;
pub mod document_save;
pub mod document_transaction;
pub mod file_format;
pub mod formula_coordinator;
pub mod history_restore_transaction;
pub mod layout_units;
pub mod prepared_documents;
pub mod projection_codec;
pub mod projection_limits;
pub mod projection_mapper;
pub mod rich_projection;
#[cfg(any(target_os = "android", target_os = "ios", test))]
pub mod transient_files;
pub mod workbook_state;

pub mod platform {
    #[cfg(target_os = "android")]
    pub mod android;
    #[cfg(desktop)]
    pub mod desktop;
    #[cfg(target_os = "ios")]
    pub mod ios;
    #[cfg(any(target_os = "android", target_os = "ios", test))]
    pub mod mobile;
}
