pub mod codec {
    pub(crate) mod address;
    pub mod reader;
    pub mod writer;
}

#[cfg(not(target_arch = "wasm32"))]
pub mod atomic_file;
pub mod input_limits;
pub mod layout_units;
pub mod projection_codec;
pub mod projection_mapper;
