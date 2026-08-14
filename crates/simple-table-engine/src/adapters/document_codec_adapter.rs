use std::sync::Arc;

use crate::application::document_codec_port::{
    DocumentCodecPort, DocumentDecodePlan, OpenDocumentSource, SavedDocumentDecodePlan,
};
use crate::application::document_encode_port::DocumentEncodePort;
use crate::document::backing::document_body::SpreadsheetDocumentBody;
use crate::document::document_model::SpreadsheetDocument;
use crate::document::document_save::{DocumentSaveEncoding, SpreadsheetDocumentSaveSnapshot};
use crate::document_data::DocumentData;
use crate::document_format::{file_name_from_path_like, open_extension_from_path_name_or_bytes};
use crate::error::AppError;
use crate::io::codec::reader::{
    InputFilePreflight, preflight_input_file, read_file_with_workbook_from_preflight,
};
use crate::io::codec::writer;
use crate::io::projection_codec::WorkbookProjectionCodec;
use crate::state::editor_state::EditorState;
use umya_spreadsheet::Workbook;

#[derive(Default)]
pub(crate) struct DocumentCodecAdapter;

struct IoDocumentDecodePlan {
    preflight: InputFilePreflight,
}

struct IoSavedDocumentDecodePlan {
    preflight: InputFilePreflight,
}

fn document_body(
    projection: &DocumentData,
    workbook: Option<Workbook>,
    source_excel_bytes: Option<Arc<[u8]>>,
) -> SpreadsheetDocumentBody {
    match (workbook, source_excel_bytes) {
        (Some(workbook), Some(source_bytes)) => SpreadsheetDocumentBody::from_opened_workbook(
            workbook,
            source_bytes,
            projection,
            Arc::new(WorkbookProjectionCodec),
        ),
        (Some(workbook), None) => {
            SpreadsheetDocumentBody::from_workbook(workbook, Arc::new(WorkbookProjectionCodec))
        }
        (None, _) => SpreadsheetDocumentBody::from_projection(projection),
    }
}

impl DocumentDecodePlan for IoDocumentDecodePlan {
    fn estimated_parse_bytes(&self) -> usize {
        self.preflight.estimated_parse_bytes()
    }

    fn decode(self: Box<Self>, source: OpenDocumentSource) -> Result<EditorState, AppError> {
        let resolved_file_name = source
            .file_name
            .unwrap_or_else(|| file_name_from_path_like(&source.path, "unknown"));
        let result = read_file_with_workbook_from_preflight(
            self.preflight,
            source.bytes,
            source.path,
            resolved_file_name,
        )?;
        let body = document_body(
            &result.file_data,
            result.workbook,
            result.source_excel_bytes,
        );
        Ok(EditorState::from_document(
            SpreadsheetDocument::from_backing(result.file_data, body),
        ))
    }
}

impl SavedDocumentDecodePlan for IoSavedDocumentDecodePlan {
    fn estimated_parse_bytes(&self) -> usize {
        self.preflight.estimated_parse_bytes()
    }

    fn decode(
        self: Box<Self>,
        bytes: Vec<u8>,
        path: String,
        file_name: String,
    ) -> Result<SpreadsheetDocument, AppError> {
        let result =
            read_file_with_workbook_from_preflight(self.preflight, bytes, path, file_name)?;
        let body = document_body(
            &result.file_data,
            result.workbook,
            result.source_excel_bytes,
        );
        Ok(SpreadsheetDocument::from_backing(result.file_data, body))
    }
}

impl DocumentEncodePort for DocumentCodecAdapter {
    fn encode(
        &self,
        snapshot: &SpreadsheetDocumentSaveSnapshot,
        target_path_or_name: &str,
    ) -> Result<(String, Vec<u8>), AppError> {
        match snapshot.encoding()? {
            DocumentSaveEncoding::NativeBytes(bytes) => {
                writer::native_excel_bytes_for_target(bytes, target_path_or_name)
            }
            DocumentSaveEncoding::NativeWorkbook(workbook) => {
                writer::generate_excel_bytes_from_workbook_for_target(workbook, target_path_or_name)
            }
            DocumentSaveEncoding::Projection(projection) => {
                writer::generate_file_bytes_for_target(projection, target_path_or_name)
            }
        }
    }
}

impl DocumentCodecPort for DocumentCodecAdapter {
    fn create_document(&self, projection: DocumentData) -> Result<EditorState, AppError> {
        let workbook = writer::workbook_from_file_data(&projection)?;
        let body =
            SpreadsheetDocumentBody::from_workbook(workbook, Arc::new(WorkbookProjectionCodec));
        Ok(EditorState::from_document(
            SpreadsheetDocument::from_backing(projection, body),
        ))
    }

    fn plan_open(
        &self,
        source: &OpenDocumentSource,
    ) -> Result<Box<dyn DocumentDecodePlan>, AppError> {
        let extension = open_extension_from_path_name_or_bytes(
            &source.path,
            source.file_name.as_deref(),
            &source.bytes,
        );
        Ok(Box::new(IoDocumentDecodePlan {
            preflight: preflight_input_file(&extension, &source.bytes)?,
        }))
    }

    fn plan_saved(
        &self,
        extension: &str,
        bytes: &[u8],
    ) -> Result<Box<dyn SavedDocumentDecodePlan>, AppError> {
        Ok(Box::new(IoSavedDocumentDecodePlan {
            preflight: preflight_input_file(extension, bytes)?,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document_data::{DocumentData, DocumentSheet};
    use crate::domain::{CellValue, EditorCommand};
    use std::io::{Cursor, Read};

    fn workbook_bytes(workbook: &Workbook) -> Vec<u8> {
        let mut bytes = Vec::new();
        umya_spreadsheet::writer::xlsx::write_writer(workbook, &mut bytes).expect("write workbook");
        bytes
    }

    fn archive_entry(bytes: &[u8], name: &str) -> Vec<u8> {
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).expect("open archive");
        let mut entry = archive.by_name(name).expect("find archive entry");
        let mut content = Vec::new();
        entry.read_to_end(&mut content).expect("read archive entry");
        content
    }

    fn open_excel(adapter: &DocumentCodecAdapter, file_name: &str, bytes: Vec<u8>) -> EditorState {
        let source = OpenDocumentSource {
            path: format!("/tmp/{file_name}"),
            bytes,
            file_name: Some(file_name.to_string()),
        };
        adapter
            .plan_open(&source)
            .expect("plan Excel open")
            .decode(source)
            .expect("decode Excel workbook")
    }

    #[test]
    fn extensionless_csv_source_is_decoded_through_the_port() {
        let adapter = DocumentCodecAdapter;
        let source = OpenDocumentSource {
            path: "/tmp/imported".to_string(),
            bytes: b"name,score\nalice,42".to_vec(),
            file_name: Some("imported".to_string()),
        };
        let plan = adapter.plan_open(&source).expect("plan CSV");
        let state = plan.decode(source).expect("decode CSV");
        let rows = &state.file_data().sheets[0].rows;

        assert_eq!(rows[0][0], CellValue::String("name".to_string()));
        assert_eq!(rows[1][1], CellValue::Number(42.into()));
    }

    #[test]
    fn unchanged_excel_save_returns_the_original_bytes() {
        let mut workbook = umya_spreadsheet::new_file();
        workbook
            .sheet_mut(0)
            .expect("sheet")
            .cell_mut("A1")
            .set_value_string("unchanged");
        let source_bytes = workbook_bytes(&workbook);
        let adapter = DocumentCodecAdapter;
        let state = open_excel(&adapter, "book.xlsx", source_bytes.clone());
        let snapshot = state
            .save_snapshot_for_target("book.xlsx")
            .expect("snapshot unchanged workbook");

        let (_, saved_bytes) = adapter
            .encode(&snapshot, "book.xlsx")
            .expect("encode unchanged workbook");

        assert_eq!(saved_bytes, source_bytes);
    }

    #[test]
    fn changed_excel_save_keeps_unchanged_sheets_on_umya_raw_path() {
        let mut workbook = umya_spreadsheet::new_file();
        workbook
            .sheet_mut(0)
            .expect("first sheet")
            .cell_mut("A1")
            .set_value_string("before");
        workbook
            .new_sheet("Untouched")
            .expect("second sheet")
            .cell_mut("B2")
            .set_value_string("keep raw");
        let source_bytes = workbook_bytes(&workbook);
        let untouched_xml = archive_entry(&source_bytes, "xl/worksheets/sheet2.xml");
        let adapter = DocumentCodecAdapter;
        let mut state = open_excel(&adapter, "book.xlsx", source_bytes);
        state
            .execute(EditorCommand::SetCell {
                sheet_index: 0,
                row: 0,
                col: 0,
                text: "after".to_string(),
            })
            .expect("edit first sheet");
        let snapshot = state
            .save_snapshot_for_target("book.xlsx")
            .expect("snapshot changed workbook");

        let (_, saved_bytes) = adapter
            .encode(&snapshot, "book.xlsx")
            .expect("encode changed workbook");
        let saved = umya_spreadsheet::reader::xlsx::read_reader(Cursor::new(&saved_bytes), true)
            .expect("read saved workbook");

        assert_eq!(
            saved
                .sheet(0)
                .expect("first sheet")
                .cell("A1")
                .expect("A1")
                .value(),
            "after"
        );
        assert_eq!(
            archive_entry(&saved_bytes, "xl/worksheets/sheet2.xml"),
            untouched_xml
        );
    }

    #[test]
    fn changed_formula_result_is_written_instead_of_using_raw_sheet_data() {
        let mut workbook = umya_spreadsheet::new_file();
        workbook
            .sheet_mut(0)
            .expect("source sheet")
            .cell_mut("A1")
            .set_value_number(1);
        let formula_sheet = workbook.new_sheet("Calc").expect("formula sheet");
        formula_sheet.cell_mut("A1").set_formula("Sheet1!A1+1");
        formula_sheet.cell_mut("A1").set_formula_result_number(2);
        let source_bytes = workbook_bytes(&workbook);
        let adapter = DocumentCodecAdapter;
        let mut state = open_excel(&adapter, "formula.xlsx", source_bytes);
        state
            .execute(EditorCommand::SetCell {
                sheet_index: 0,
                row: 0,
                col: 0,
                text: "2".to_string(),
            })
            .expect("edit formula input");
        let snapshot = state
            .save_snapshot_for_target("formula.xlsx")
            .expect("snapshot recalculated workbook");

        let (_, saved_bytes) = adapter
            .encode(&snapshot, "formula.xlsx")
            .expect("encode recalculated workbook");
        let saved = umya_spreadsheet::reader::xlsx::read_reader(Cursor::new(saved_bytes), true)
            .expect("read recalculated workbook");

        assert_eq!(
            saved
                .sheet(1)
                .expect("formula sheet")
                .cell("A1")
                .expect("A1")
                .value(),
            "3"
        );
    }

    #[test]
    fn xlsm_save_preserves_macros_through_umya() {
        let macros = b"test vba project bytes".to_vec();
        let mut workbook = umya_spreadsheet::new_file();
        workbook.set_macros_code(macros.clone());
        workbook
            .sheet_mut(0)
            .expect("sheet")
            .cell_mut("A1")
            .set_value_string("before");
        let source_bytes = workbook_bytes(&workbook);
        let adapter = DocumentCodecAdapter;
        let mut state = open_excel(&adapter, "macro.xlsm", source_bytes);
        state
            .execute(EditorCommand::SetCell {
                sheet_index: 0,
                row: 0,
                col: 0,
                text: "after".to_string(),
            })
            .expect("edit macro workbook");
        let snapshot = state
            .save_snapshot_for_target("macro.xlsm")
            .expect("snapshot macro workbook");

        let (output_name, saved_bytes) = adapter
            .encode(&snapshot, "macro.xlsm")
            .expect("encode macro workbook");
        let saved = umya_spreadsheet::reader::xlsx::read_reader(Cursor::new(saved_bytes), true)
            .expect("read saved macro workbook");

        assert_eq!(output_name, "macro.xlsm");
        assert_eq!(saved.macros_code(), Some(macros.as_slice()));
    }

    #[test]
    fn new_xlsx_document_has_native_image_capabilities() {
        let state = DocumentCodecAdapter
            .create_document(DocumentData {
                path: String::new(),
                file_name: "untitled.xlsx".to_string(),
                sheets: vec![DocumentSheet {
                    name: "Sheet1".to_string(),
                    rows: vec![vec![CellValue::Null; 5]; 5],
                    ..Default::default()
                }],
            })
            .expect("create XLSX document");

        assert!(state.can_finish_save_without_reparse(true));
        assert!(state.capabilities().rich.images.can_insert);
    }

    #[test]
    fn projection_snapshot_is_encoded_through_the_port() {
        let document = SpreadsheetDocument::new(DocumentData {
            path: String::new(),
            file_name: "source.csv".to_string(),
            sheets: vec![DocumentSheet {
                rows: vec![vec![CellValue::String("encoded".to_string())]],
                ..Default::default()
            }],
        });
        let snapshot = document
            .save_snapshot_for_target("export.csv")
            .expect("save snapshot");

        let (output_name, bytes) = DocumentCodecAdapter
            .encode(&snapshot, "export.csv")
            .expect("encode snapshot");

        assert_eq!(output_name, "export.csv");
        assert_eq!(String::from_utf8(bytes).expect("UTF-8 CSV"), "encoded\n");
    }

    #[test]
    fn saved_document_decode_reuses_its_preflight_plan() {
        let adapter = DocumentCodecAdapter;
        let bytes = b"name,score\nalice,42".to_vec();
        let plan = adapter.plan_saved("csv", &bytes).expect("plan saved CSV");

        assert_eq!(plan.estimated_parse_bytes(), bytes.len() * 3);
        let document = plan
            .decode(bytes, "/tmp/saved.csv".to_string(), "saved.csv".to_string())
            .expect("decode saved CSV");

        assert_eq!(
            document.projection().sheets[0].rows[1][1],
            CellValue::Number(42.into())
        );
    }
}
