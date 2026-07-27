pub(crate) mod document_codec_adapter;
pub mod document_file_adapter;
pub(crate) mod document_work_budget_adapter;
pub mod recent_file_adapter;
pub(crate) mod search_document_source_adapter;
pub mod search_index_adapter;
pub(crate) mod search_index_runtime;
mod search_index_scheduler;
pub(crate) mod search_index_store;
mod search_index_worker;
pub(crate) mod search_query_adapter;
pub(crate) mod search_query_engine;
pub(crate) mod search_text_analyzer;
#[cfg(any(target_os = "android", target_os = "ios", test))]
pub mod update_adapter;
