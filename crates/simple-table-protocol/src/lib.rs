mod dto;

use serde::{Deserialize, Serialize};

pub use dto::*;

pub const SHEET_REGION_TILE_ROWS: usize = 128;
pub const SHEET_REGION_TILE_COLUMNS: usize = 32;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CellEdit {
    pub sheet_index: usize,
    pub row: usize,
    pub col: usize,
    pub text: String,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SortDirectionDto {
    Ascending,
    Descending,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum FilterOperatorDto {
    Equals,
    NotEquals,
    Contains,
    Blank,
    NotBlank,
}

pub type ImageMarkerDto = ImageMarker;
pub type ImageAnchorDto = ImageAnchor;
pub type SheetImageDto = SheetImage;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum EditorRequest {
    NewDocument {
        request_id: String,
    },
    OpenDocument {
        request_id: String,
        file_name: String,
    },
    OpenRecoveryDocument {
        request_id: String,
        file_name: String,
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
    RowsRegion {
        document_id: u64,
        base_revision: u64,
        sheet_index: usize,
        rows: Vec<usize>,
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
    SortRows {
        request_id: String,
        document_id: u64,
        base_revision: u64,
        sheet_index: usize,
        anchor_row: usize,
        anchor_col: usize,
        direction: SortDirectionDto,
    },
    SetFilter {
        request_id: String,
        document_id: u64,
        base_revision: u64,
        sheet_index: usize,
        anchor_row: usize,
        col: usize,
        operator: FilterOperatorDto,
        value: String,
    },
    ClearFilter {
        request_id: String,
        document_id: u64,
        base_revision: u64,
        sheet_index: usize,
        col: Option<usize>,
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
        value: Option<OpenDocumentResponse>,
    },
    Region {
        value: SheetRegionProjectionResponse,
    },
    RowsRegion {
        value: SheetRowsRegionProjectionResponse,
    },
    Mutation {
        value: EditorMutationResponse,
    },
    Search {
        value: SearchResponse,
    },
    Images {
        items: Vec<SheetImageDto>,
        next_offset: Option<usize>,
    },
    Bytes,
    SavePrepared {
        save_token: String,
        file_name: String,
    },
    ExportPrepared {
        file_name: String,
    },
    Saved {
        value: SavedDocumentResponse,
    },
    Closed,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppErrorDto {
    pub code: String,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EditorCommand {
    pub request: EditorRequest,
    pub attachment: Option<Vec<u8>>,
}

impl EditorCommand {
    pub fn new(request: EditorRequest) -> Self {
        Self {
            request,
            attachment: None,
        }
    }

    pub fn with_attachment(request: EditorRequest, attachment: Vec<u8>) -> Self {
        Self {
            request,
            attachment: Some(attachment),
        }
    }
}

impl From<EditorRequest> for EditorCommand {
    fn from(request: EditorRequest) -> Self {
        Self::new(request)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EditorOutput {
    pub reply: EditorReply,
    pub attachment: Option<Vec<u8>>,
}

impl EditorOutput {
    pub fn new(reply: EditorReply) -> Self {
        Self {
            reply,
            attachment: None,
        }
    }

    pub fn with_attachment(reply: EditorReply, attachment: Vec<u8>) -> Self {
        Self {
            reply,
            attachment: Some(attachment),
        }
    }
}

pub type EditorResponse = Result<EditorOutput, AppErrorDto>;

pub(crate) mod u64_string {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(value: &u64, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&value.to_string())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<u64, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(serde::de::Error::custom)
    }
}

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
    fn sparse_rows_region_round_trip_keeps_physical_row_indexes() {
        let request = EditorRequest::RowsRegion {
            document_id: 9,
            base_revision: 4,
            sheet_index: 2,
            rows: vec![0, 1_024, 249_999],
            col_start: 3,
            col_end: 35,
        };
        let json = serde_json::to_string(&request).expect("serialize");

        assert_eq!(
            serde_json::from_str::<EditorRequest>(&json).expect("deserialize"),
            request
        );
    }

    #[test]
    fn protocol_wire_shape_stays_stable() {
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

        let response: Result<EditorReply, AppErrorDto> = Err(AppErrorDto {
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
