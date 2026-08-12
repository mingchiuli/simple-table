pub mod protocol;

#[cfg(feature = "engine")]
pub(crate) mod adapters;
#[cfg(feature = "engine")]
pub(crate) mod application;
#[cfg(feature = "engine")]
pub(crate) mod document;
#[cfg(feature = "engine")]
pub(crate) mod document_data;
#[cfg(feature = "engine")]
pub(crate) mod document_format;
#[cfg(feature = "engine")]
pub(crate) mod document_layout_policy;
#[cfg(feature = "engine")]
pub(crate) mod document_resource_estimator;
#[cfg(feature = "engine")]
pub(crate) mod domain;
#[cfg(feature = "engine")]
pub(crate) mod error;
#[cfg(feature = "engine")]
mod facade;
#[cfg(feature = "engine")]
pub(crate) mod formula;
#[cfg(feature = "engine")]
pub(crate) mod io;
#[cfg(feature = "engine")]
pub(crate) mod ops;
#[cfg(feature = "engine")]
pub(crate) mod projection_model;
#[cfg(feature = "engine")]
pub(crate) mod protocol_projection;
#[cfg(feature = "engine")]
pub(crate) mod resource_limits;
#[cfg(feature = "engine")]
pub(crate) mod runtime;
#[cfg(feature = "engine")]
pub(crate) mod state;
#[cfg(feature = "engine")]
pub(crate) mod types;

#[cfg(feature = "engine")]
pub use facade::CoreFacade;
#[cfg(feature = "engine")]
pub use types::*;

#[cfg(all(feature = "engine", not(target_arch = "wasm32")))]
pub fn write_native_file_atomically(
    path: &std::path::Path,
    bytes: &[u8],
) -> Result<(), crate::protocol::AppErrorDto> {
    io::atomic_file::write_file_atomically(path, bytes).map_err(crate::protocol::AppErrorDto::from)
}
