pub mod codec {
    pub mod reader;
    pub mod writer;
}

pub mod atomic_file;
pub mod document_body;
pub mod input_limits;
pub mod layout_units;
#[cfg(any(target_os = "android", target_os = "ios", test))]
pub mod managed_documents;
#[cfg(any(target_os = "android", target_os = "ios", test))]
pub mod marker_store;
pub mod open_file_input;
pub mod projection_codec;
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
