pub mod codec {
    pub mod reader;
    pub mod writer;
}

pub mod document;
pub mod document_body;
pub mod document_model;
pub mod document_patches;
pub mod workbook_state;

pub mod platform {
    #[cfg(target_os = "android")]
    pub mod android;
    #[cfg(desktop)]
    pub mod desktop;
    #[cfg(target_os = "ios")]
    pub mod ios;
    #[cfg(any(target_os = "android", target_os = "ios"))]
    pub mod mobile;
}
