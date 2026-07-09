use std::path::Path;

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
    Path::new(path_or_name)
        .extension()
        .and_then(|extension| extension.to_str())
        .filter(|extension| !extension.is_empty())
        .map(|extension| extension.to_ascii_lowercase())
}

pub fn supported_extension_from_name(file_name: &str) -> Option<String> {
    extension_of(file_name)
        .and_then(|extension| SpreadsheetFileFormat::from_extension(&extension))
        .map(|format| format.extension().to_string())
}

pub fn extension_or_default(file_name: &str) -> String {
    extension_of(file_name).unwrap_or_else(|| DEFAULT_SPREADSHEET_FORMAT.extension().to_string())
}

pub fn export_extensions() -> Vec<String> {
    SUPPORTED_SPREADSHEET_EXTENSIONS
        .iter()
        .map(|extension| (*extension).to_string())
        .collect()
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
}
