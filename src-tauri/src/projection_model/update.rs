#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MobileUpdateSnapshot {
    pub version: String,
    pub tag_name: String,
    pub release_url: String,
    pub apk_url: Option<String>,
}
