pub(crate) mod document_codec_adapter;
pub(crate) mod document_work_budget_adapter;
pub(crate) mod search_document_source_adapter;
// Target-specific search implementations are aliased to shared module names so
// call sites stay free of `cfg(target_arch = "wasm32")` branching.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod search_index_adapter_native;
#[cfg(target_arch = "wasm32")]
pub(crate) mod search_index_adapter_web;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use search_index_adapter_native as search_index_adapter;
#[cfg(target_arch = "wasm32")]
pub(crate) use search_index_adapter_web as search_index_adapter;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod search_index_backend_native;
#[cfg(target_arch = "wasm32")]
pub(crate) mod search_index_backend_web;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use search_index_backend_native as search_index_backend;
#[cfg(target_arch = "wasm32")]
pub(crate) use search_index_backend_web as search_index_backend;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod search_index_registry;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod search_index_runtime;
#[cfg(not(target_arch = "wasm32"))]
mod search_index_scheduler;
#[cfg(not(target_arch = "wasm32"))]
mod search_index_worker;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod search_query_adapter_native;
#[cfg(target_arch = "wasm32")]
pub(crate) mod search_query_adapter_web;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use search_query_adapter_native as search_query_adapter;
#[cfg(target_arch = "wasm32")]
pub(crate) use search_query_adapter_web as search_query_adapter;
pub(crate) mod search_query_engine;
