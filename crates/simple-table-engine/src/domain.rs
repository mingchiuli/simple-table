mod cell_display;
pub(crate) mod cell_key;
mod cell_value;
mod document_change;
mod editor_operation;
mod search_document;
mod search_index_work;
mod search_query;
mod table_operation;

pub(crate) use cell_display::{format_cell_display, format_cell_search};
pub use cell_value::{CellNumber, CellValue, parse_cell_text};
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
pub use table_operation::{CellRange, FilterOperator, ResolvedSort, SortDirection};
pub(crate) use table_operation::{
    FormulaTextAtCell, apply_sort_to_projection, current_region, resolve_sort,
};
