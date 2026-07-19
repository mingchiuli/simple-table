#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenFileSelection {
    pub path: String,
    pub original_path: String,
    pub file_name: String,
}

pub struct OpenFileInput {
    pub path: String,
    pub bytes: Vec<u8>,
    pub file_name: Option<String>,
}
