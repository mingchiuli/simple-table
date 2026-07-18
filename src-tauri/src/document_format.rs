use std::path::Path;

use crate::types::SpreadsheetFormatOptions;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpreadsheetFileFormat {
    Xlsx,
    Csv,
}

pub const DEFAULT_SPREADSHEET_FORMAT: SpreadsheetFileFormat = SpreadsheetFileFormat::Xlsx;
pub const SUPPORTED_SPREADSHEET_EXTENSIONS: &[&str] = &["xlsx", "csv"];

impl SpreadsheetFileFormat {
    pub fn from_extension(extension: &str) -> Option<Self> {
        match extension.to_ascii_lowercase().as_str() {
            "xlsx" => Some(Self::Xlsx),
            "csv" => Some(Self::Csv),
            _ => None,
        }
    }

    pub fn from_path_or_default(path_or_name: &str) -> Option<Self> {
        match extension_of(path_or_name) {
            Some(extension) => Self::from_extension(&extension),
            None => Some(DEFAULT_SPREADSHEET_FORMAT),
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Xlsx => "xlsx",
            Self::Csv => "csv",
        }
    }

    pub fn is_xlsx(self) -> bool {
        matches!(self, Self::Xlsx)
    }
}

pub fn extension_of(path_or_name: &str) -> Option<String> {
    let file_name = file_name_from_path_like(path_or_name, "");
    Path::new(&file_name)
        .extension()
        .and_then(|extension| extension.to_str())
        .filter(|extension| !extension.is_empty())
        .map(|extension| extension.to_ascii_lowercase())
}

pub fn file_name_from_path_like(path_or_name: &str, fallback: &str) -> String {
    let without_hash = path_or_name.split('#').next().unwrap_or(path_or_name);
    let without_query = without_hash.split('?').next().unwrap_or(without_hash);
    let normalized = without_query.replace('\\', "/");
    let segment = normalized
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or_default();
    let decoded = decode_percent_segment(segment);
    let decoded_normalized = decoded.replace('\\', "/");
    let decoded_segment = decoded_normalized
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or_default();

    if decoded_segment.is_empty() {
        fallback.to_string()
    } else {
        decoded_segment.to_string()
    }
}

pub fn file_stem_from_path_like(path_or_name: &str, fallback: &str) -> String {
    let file_name = file_name_from_path_like(path_or_name, "");
    let stem = Path::new(&file_name)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty());
    stem.unwrap_or(fallback).to_string()
}

fn decode_percent_segment(segment: &str) -> String {
    let bytes = segment.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let (Some(high), Some(low)) =
                (hex_value(bytes[index + 1]), hex_value(bytes[index + 2]))
        {
            decoded.push((high << 4) | low);
            index += 3;
            continue;
        }
        decoded.push(bytes[index]);
        index += 1;
    }

    String::from_utf8(decoded).unwrap_or_else(|_| segment.to_string())
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

pub fn default_spreadsheet_extension() -> &'static str {
    DEFAULT_SPREADSHEET_FORMAT.extension()
}

pub fn default_spreadsheet_file_name(stem: &str) -> String {
    format!("{stem}.{}", default_spreadsheet_extension())
}

pub fn supported_extension_from_name(file_name: &str) -> Option<String> {
    extension_of(file_name)
        .and_then(|extension| SpreadsheetFileFormat::from_extension(&extension))
        .map(|format| format.extension().to_string())
}

pub fn is_xlsx_extension(extension: &str) -> bool {
    SpreadsheetFileFormat::from_extension(extension).is_some_and(SpreadsheetFileFormat::is_xlsx)
}

pub fn open_extension_from_path_name_or_bytes(
    path: &str,
    file_name: Option<&str>,
    bytes: &[u8],
) -> String {
    let path_extension = extension_of(path);
    if let Some(extension) = supported_extension(path_extension.as_deref()) {
        return extension;
    }

    let file_name_extension = file_name.and_then(extension_of);
    if let Some(extension) = supported_extension(file_name_extension.as_deref()) {
        return extension;
    }

    if path_extension.is_none()
        && file_name_extension.is_none()
        && let Some(format) = detect_extensionless_format_from_bytes(bytes)
    {
        return format.extension().to_string();
    }

    path_extension
        .or(file_name_extension)
        .unwrap_or_else(|| DEFAULT_SPREADSHEET_FORMAT.extension().to_string())
}

#[cfg(any(test, target_os = "android", target_os = "ios"))]
pub fn supported_extension_or_default(file_name: &str) -> String {
    SpreadsheetFileFormat::from_path_or_default(file_name)
        .unwrap_or(DEFAULT_SPREADSHEET_FORMAT)
        .extension()
        .to_string()
}

#[cfg(any(test, target_os = "android", target_os = "ios"))]
pub fn import_extension_from_name_or_bytes(file_name: &str, bytes: &[u8]) -> Option<String> {
    if let Some(extension) = supported_extension_from_name(file_name) {
        return Some(extension);
    }

    if extension_of(file_name).is_some() {
        return None;
    }

    detect_extensionless_format_from_bytes(bytes).map(|format| format.extension().to_string())
}

#[cfg(any(test, target_os = "android", target_os = "ios"))]
pub fn normalized_import_file_name(file_name: &str, extension: &str) -> String {
    if supported_extension_from_name(file_name).is_some() {
        file_name.to_string()
    } else {
        format!("imported.{extension}")
    }
}

pub fn export_extensions() -> Vec<String> {
    SUPPORTED_SPREADSHEET_EXTENSIONS
        .iter()
        .map(|extension| (*extension).to_string())
        .collect()
}

pub fn spreadsheet_format_options() -> SpreadsheetFormatOptions {
    SpreadsheetFormatOptions {
        default_extension: default_spreadsheet_extension().to_string(),
        supported_extensions: export_extensions(),
    }
}

#[cfg_attr(
    not(any(test, target_os = "android", target_os = "ios")),
    allow(dead_code)
)]
pub fn output_name_for_selected_target(selected_name: Option<&str>, fallback_name: &str) -> String {
    if let Some(selected_name) =
        selected_name.filter(|name| supported_extension_from_name(name).is_some())
    {
        return selected_name.to_string();
    }

    if SpreadsheetFileFormat::from_path_or_default(fallback_name).is_some() {
        return fallback_name.to_string();
    }

    default_spreadsheet_file_name("export")
}

fn supported_extension(extension: Option<&str>) -> Option<String> {
    extension
        .and_then(SpreadsheetFileFormat::from_extension)
        .map(|format| format.extension().to_string())
}

fn detect_extensionless_format_from_bytes(bytes: &[u8]) -> Option<SpreadsheetFileFormat> {
    if bytes.starts_with(b"PK") {
        return Some(SpreadsheetFileFormat::Xlsx);
    }

    if std::str::from_utf8(bytes).is_ok() {
        return Some(SpreadsheetFileFormat::Csv);
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_supported_extensions_case_insensitively() {
        assert_eq!(
            SpreadsheetFileFormat::from_extension("XLSX"),
            Some(SpreadsheetFileFormat::Xlsx)
        );
        assert_eq!(
            SpreadsheetFileFormat::from_extension("csv"),
            Some(SpreadsheetFileFormat::Csv)
        );
        assert_eq!(SpreadsheetFileFormat::from_extension("xlsm"), None);
        assert_eq!(default_spreadsheet_extension(), "xlsx");
        assert_eq!(default_spreadsheet_file_name("book"), "book.xlsx");
        assert!(is_xlsx_extension("XLSX"));
        assert!(!is_xlsx_extension("csv"));
        assert!(!is_xlsx_extension("xlsm"));
    }

    #[test]
    fn extracts_file_name_from_path_like_inputs() {
        assert_eq!(
            file_name_from_path_like(r"C:\Users\me\book.xlsx", "fallback.xlsx"),
            "book.xlsx"
        );
        assert_eq!(
            file_name_from_path_like(
                "content://provider/document/primary%3ADownload%2Freports%2Fscore.xlsx?x=1#top",
                "fallback.xlsx"
            ),
            "score.xlsx"
        );
        assert_eq!(
            file_name_from_path_like(
                "content://provider/document/%E7%BB%A9%E6%95%88.xlsx",
                "fallback.xlsx"
            ),
            "绩效.xlsx"
        );
        assert_eq!(
            file_name_from_path_like(
                "content://provider/document/bad%ZZname.xlsx",
                "fallback.xlsx"
            ),
            "bad%ZZname.xlsx"
        );
        assert_eq!(
            file_name_from_path_like("content://provider/document/", "fallback.xlsx"),
            "document"
        );
    }

    #[test]
    fn extracts_extensions_from_path_like_inputs() {
        assert_eq!(
            extension_of(r"C:\Users\me\book.XLSX"),
            Some("xlsx".to_string())
        );
        assert_eq!(
            extension_of(
                "content://provider/document/primary%3ADownload%2Freports%2Fscore.CSV?x=1#top"
            ),
            Some("csv".to_string())
        );
        assert_eq!(
            extension_of("content://provider/document/bad%ZZname.xlsx"),
            Some("xlsx".to_string())
        );
        assert_eq!(extension_of("content://provider/document/"), None);
    }

    #[test]
    fn extracts_stems_from_path_like_inputs() {
        assert_eq!(
            file_stem_from_path_like(r"C:\Users\me\book.xlsx", "untitled"),
            "book"
        );
        assert_eq!(
            file_stem_from_path_like(
                "content://provider/document/primary%3ADownload%2Freports%2Fscore.final.xlsx?x=1",
                "untitled"
            ),
            "score.final"
        );
        assert_eq!(
            file_stem_from_path_like("content://provider/document/", "untitled"),
            "document"
        );
    }

    #[test]
    fn exposes_format_options_from_the_same_backend_source() {
        assert_eq!(
            spreadsheet_format_options(),
            SpreadsheetFormatOptions {
                default_extension: "xlsx".to_string(),
                supported_extensions: vec!["xlsx".to_string(), "csv".to_string()],
            }
        );
    }

    #[test]
    fn selected_output_name_uses_selected_supported_extension_or_fallback() {
        assert_eq!(
            output_name_for_selected_target(Some("chosen.csv"), "default.xlsx"),
            "chosen.csv"
        );
        assert_eq!(
            output_name_for_selected_target(Some("content://provider/document/42"), "default.csv"),
            "default.csv"
        );
        assert_eq!(
            output_name_for_selected_target(Some("unsupported.bin"), "default.xlsx"),
            "default.xlsx"
        );
        assert_eq!(
            output_name_for_selected_target(Some("unsupported.bin"), "default.bin"),
            "export.xlsx"
        );
    }

    #[test]
    fn defaults_extensionless_names_to_xlsx() {
        assert_eq!(
            SpreadsheetFileFormat::from_path_or_default("untitled"),
            Some(SpreadsheetFileFormat::Xlsx)
        );
        assert_eq!(
            SpreadsheetFileFormat::from_path_or_default("book.bin"),
            None
        );
        assert_eq!(supported_extension_from_name("book.bin"), None);
    }

    #[test]
    fn resolves_open_extension_from_path_name_or_extensionless_bytes() {
        assert_eq!(
            open_extension_from_path_name_or_bytes("/tmp/data.csv", Some("book.xlsx"), b"PK"),
            "csv"
        );
        assert_eq!(
            open_extension_from_path_name_or_bytes("/tmp/imported.tmp", Some("book.xlsx"), b"PK"),
            "xlsx"
        );
        assert_eq!(
            open_extension_from_path_name_or_bytes("/tmp/imported", None, b"PK\x03\x04"),
            "xlsx"
        );
        assert_eq!(
            open_extension_from_path_name_or_bytes("/tmp/imported", None, b"a,b"),
            "csv"
        );
        assert_eq!(
            open_extension_from_path_name_or_bytes("/tmp/imported.bin", None, b"PK\x03\x04"),
            "bin"
        );
    }

    #[test]
    fn resolves_supported_extension_or_default_without_leaking_unknown_extensions() {
        assert_eq!(supported_extension_or_default("book.xlsx"), "xlsx");
        assert_eq!(supported_extension_or_default("DATA.CSV"), "csv");
        assert_eq!(supported_extension_or_default("untitled"), "xlsx");
        assert_eq!(supported_extension_or_default("book.bin"), "xlsx");
    }

    #[test]
    fn detects_import_extension_from_supported_name_or_extensionless_bytes() {
        assert_eq!(
            import_extension_from_name_or_bytes("book.xlsx", b"ignored"),
            Some("xlsx".to_string())
        );
        assert_eq!(
            import_extension_from_name_or_bytes("data.csv", b"ignored"),
            Some("csv".to_string())
        );
        assert_eq!(
            import_extension_from_name_or_bytes("unknown", b"PK\x03\x04"),
            Some("xlsx".to_string())
        );
        assert_eq!(
            import_extension_from_name_or_bytes("unknown", b"a,b"),
            Some("csv".to_string())
        );
        assert_eq!(
            import_extension_from_name_or_bytes("unsupported.bin", b"PK\x03\x04"),
            None
        );
    }

    #[test]
    fn normalizes_import_name_only_when_source_name_has_no_supported_extension() {
        assert_eq!(
            normalized_import_file_name("report.csv", "csv"),
            "report.csv"
        );
        assert_eq!(normalized_import_file_name("report", "csv"), "imported.csv");
        assert_eq!(
            normalized_import_file_name("report.bin", "xlsx"),
            "imported.xlsx"
        );
    }
}
