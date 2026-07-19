mod cell_display;
pub(crate) mod cell_key;
mod cell_value;
mod document_change;
mod editor_operation;
mod search_document;
mod search_index_work;
mod search_query;

pub(crate) use cell_display::{format_cell_display, format_cell_search};
pub use cell_value::{CellNumber, CellValue, normalize_formula_text, parse_cell_text};
pub use document_change::DocumentCellChange;
pub use editor_operation::{
    AppliedOperation, CellEditInput, EditorCommand, MutationImpact, OperationPatchProjector,
    ProjectionMutation, ResolvedCellEdit,
};
pub use search_document::{
    SearchCellText, SearchDocumentSnapshot, SearchScanCursor, SearchSheetSnapshot, SearchTextChunk,
};
pub use search_index_work::{SearchCellIndexUpdate, SearchIndexWork};
pub(crate) use search_query::{SearchHit, SearchOutcome, SearchScope};
