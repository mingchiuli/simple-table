use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const PROTOCOL_VERSION: u16 = 2;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CellEdit {
    pub sheet_index: usize,
    pub row: usize,
    pub col: usize,
    pub text: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LocalDocumentSummary {
    pub id: String,
    pub name: String,
    pub updated_at_ms: u64,
    pub has_recovery: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ImageMarkerDto {
    pub row: u32,
    pub col: u32,
    pub row_offset_emu: i32,
    pub col_offset_emu: i32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", content = "data")]
pub enum ImageAnchorDto {
    OneCell {
        from: ImageMarkerDto,
        #[serde(rename = "widthEmu")]
        width_emu: u32,
        #[serde(rename = "heightEmu")]
        height_emu: u32,
    },
    TwoCell {
        from: ImageMarkerDto,
        to: ImageMarkerDto,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SheetImageDto {
    pub id: String,
    pub media_id: String,
    pub mime_type: String,
    pub intrinsic_width: u32,
    pub intrinsic_height: u32,
    pub anchor: ImageAnchorDto,
    pub z_index: usize,
    pub renderable: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum EditorRequest {
    NewDocument {
        request_id: String,
    },
    OpenDocument {
        request_id: String,
        file_name: String,
        bytes: Vec<u8>,
    },
    OpenRecoveryDocument {
        request_id: String,
        file_name: String,
        bytes: Vec<u8>,
    },
    ActiveDocument,
    Region {
        document_id: u64,
        base_revision: u64,
        sheet_index: usize,
        row_start: usize,
        row_end: usize,
        col_start: usize,
        col_end: usize,
    },
    SetCell {
        request_id: String,
        document_id: u64,
        base_revision: u64,
        sheet_index: usize,
        row: usize,
        col: usize,
        text: String,
    },
    SetCells {
        request_id: String,
        document_id: u64,
        base_revision: u64,
        changes: Vec<CellEdit>,
    },
    AddRow {
        request_id: String,
        document_id: u64,
        base_revision: u64,
        sheet_index: usize,
        row_index: usize,
    },
    DeleteRow {
        request_id: String,
        document_id: u64,
        base_revision: u64,
        sheet_index: usize,
        row_index: usize,
    },
    AddColumn {
        request_id: String,
        document_id: u64,
        base_revision: u64,
        sheet_index: usize,
        col_index: usize,
    },
    DeleteColumn {
        request_id: String,
        document_id: u64,
        base_revision: u64,
        sheet_index: usize,
        col_index: usize,
    },
    SetColumnWidth {
        request_id: String,
        document_id: u64,
        base_revision: u64,
        sheet_index: usize,
        col_index: usize,
        width: Option<u32>,
    },
    SetRowHeight {
        request_id: String,
        document_id: u64,
        base_revision: u64,
        sheet_index: usize,
        row_index: usize,
        height: Option<u32>,
    },
    AddSheet {
        request_id: String,
        document_id: u64,
        base_revision: u64,
    },
    DeleteSheet {
        request_id: String,
        document_id: u64,
        base_revision: u64,
        sheet_index: usize,
    },
    InsertImage {
        request_id: String,
        document_id: u64,
        base_revision: u64,
        sheet_index: usize,
        row: u32,
        col: u32,
        file_name: String,
        bytes: Vec<u8>,
    },
    SheetImages {
        document_id: u64,
        base_revision: u64,
        sheet_index: usize,
        offset: usize,
        limit: usize,
    },
    ImageBytes {
        document_id: u64,
        base_revision: u64,
        sheet_index: usize,
        image_id: String,
    },
    UpdateImage {
        request_id: String,
        document_id: u64,
        base_revision: u64,
        sheet_index: usize,
        image_id: String,
        anchor: ImageAnchorDto,
    },
    DeleteImage {
        request_id: String,
        document_id: u64,
        base_revision: u64,
        sheet_index: usize,
        image_id: String,
    },
    Undo {
        request_id: String,
        document_id: u64,
        base_revision: u64,
    },
    Redo {
        request_id: String,
        document_id: u64,
        base_revision: u64,
    },
    Search {
        document_id: u64,
        base_revision: u64,
        query: String,
        current_sheet_index: Option<usize>,
        all_sheets: bool,
    },
    PrepareSave {
        request_id: String,
        document_id: u64,
        base_revision: u64,
        target_name: String,
    },
    PrepareExport {
        document_id: u64,
        base_revision: u64,
        target_name: String,
    },
    SaveLocal {
        request_id: String,
        document_id: u64,
        base_revision: u64,
        target_name: String,
    },
    CheckpointRecovery {
        request_id: String,
        document_id: u64,
        base_revision: u64,
        target_name: String,
    },
    ClearRecovery,
    ListLocalDocuments,
    OpenLocalDocument {
        request_id: String,
        document_key: String,
    },
    DeleteLocalDocument {
        document_key: String,
    },
    CommitSave {
        save_token: String,
        path: String,
    },
    AbortSave {
        save_token: String,
    },
    CloseDocument {
        request_id: String,
        document_id: u64,
        base_revision: u64,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum EditorReply {
    Empty,
    Document {
        value: Value,
    },
    Region {
        value: Value,
    },
    Mutation {
        value: Value,
    },
    Search {
        value: Value,
    },
    Images {
        items: Vec<SheetImageDto>,
        next_offset: Option<usize>,
    },
    Bytes {
        bytes: Vec<u8>,
    },
    SavePrepared {
        save_token: String,
        file_name: String,
        bytes: Vec<u8>,
    },
    ExportPrepared {
        file_name: String,
        bytes: Vec<u8>,
    },
    Saved {
        value: Value,
    },
    LocalDocuments {
        documents: Vec<LocalDocumentSummary>,
    },
    Closed,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppErrorDto {
    pub code: String,
    pub message: String,
}

pub type EditorResponse = Result<EditorReply, AppErrorDto>;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn request_round_trip_keeps_u64_values_numeric_inside_rust_protocol() {
        let request = EditorRequest::Region {
            document_id: u64::MAX,
            base_revision: 7,
            sheet_index: 0,
            row_start: 0,
            row_end: 40,
            col_start: 0,
            col_end: 20,
        };
        let json = serde_json::to_string(&request).expect("serialize");
        assert_eq!(
            serde_json::from_str::<EditorRequest>(&json).expect("deserialize"),
            request
        );
    }

    #[test]
    fn protocol_wire_shape_stays_stable() {
        assert_eq!(PROTOCOL_VERSION, 2);
        let request = EditorRequest::SetCell {
            request_id: "edit-1".to_string(),
            document_id: 9,
            base_revision: 4,
            sheet_index: 2,
            row: 3,
            col: 5,
            text: "value".to_string(),
        };
        assert_eq!(
            serde_json::to_value(request).expect("serialize request"),
            json!({
                "type": "setCell",
                "request_id": "edit-1",
                "document_id": 9,
                "base_revision": 4,
                "sheet_index": 2,
                "row": 3,
                "col": 5,
                "text": "value",
            })
        );

        let response: EditorResponse = Err(AppErrorDto {
            code: "invalid_request".to_string(),
            message: "bad request".to_string(),
        });
        assert_eq!(
            serde_json::to_value(response).expect("serialize response"),
            json!({
                "Err": {
                    "code": "invalid_request",
                    "message": "bad request",
                }
            })
        );
    }
}
