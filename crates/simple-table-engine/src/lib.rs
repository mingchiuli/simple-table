pub(crate) use simple_table_protocol as protocol;

pub(crate) mod adapters;
pub(crate) mod application;
pub(crate) mod document;
pub(crate) mod document_data;
pub(crate) mod document_format;
pub(crate) mod document_layout_policy;
pub(crate) mod document_resource_estimator;
pub(crate) mod domain;
pub(crate) mod error;
mod facade;
pub(crate) mod formula;
pub(crate) mod io;
pub(crate) mod ops;
pub(crate) mod projection_model;
pub(crate) mod protocol_projection;
pub(crate) mod resource_limits;
pub(crate) mod runtime;
pub(crate) mod state;
pub(crate) mod types;

pub use facade::CoreFacade;
pub use types::*;

#[cfg(not(target_arch = "wasm32"))]
pub fn write_native_file_atomically(
    path: &std::path::Path,
    bytes: &[u8],
) -> Result<(), crate::protocol::AppErrorDto> {
    io::atomic_file::write_file_atomically(path, bytes).map_err(crate::protocol::AppErrorDto::from)
}
