#![allow(dead_code)]

use ts_rs::{Config, TS};

use crate::recent::types::{AddRecentFileRequest, RecentFile, StorageType};
use crate::state::state::{EditorSessionInfo, EditorStateInfo, HistoryStatus};
use crate::types::{
    CellData, CellFormatProjection, CellFormulaProjection, CellKind, CellStyleProjection,
    CellValue, ColumnDeletedPatch, ColumnInsertedPatch, DocumentCapabilities, DrawingKind,
    DrawingProjection, EditorCommandContext, EditorMutationResponse, EditorPatch, FileData,
    FormulaDiagnostics, FormulaIssue, FormulaIssueKind, FormulaStatus, FreezePaneProjection,
    HyperlinkProjection, LayoutPatch, MergeRange, NativeSavePlan, OpenDocumentResponse,
    PreparedOpenDocument, ReadOnlyRichProjection, ResyncRequiredPatch, RichProjectionPatch,
    RichProjectionPatchScope, RowDeletedPatch, RowInsertedPatch, SavedDocumentIdentity,
    SavedDocumentResponse, ScalarCellValue, SearchResult, SearchScope, SetCellRequest,
    SheetCapabilities, SheetData, SheetDeletedPatch, SheetExtent, SheetInsertedPatch,
    SheetProjectionResponse, SheetShapePatch, SheetStructureMetadataPatch, SheetUpdatedPatch,
    SheetsReplacedPatch, SpreadsheetFormatOptions, WorkbookCapabilities, WorkbookRichCapabilities,
    WorkbookSaveCapabilities, WorkbookStructureCapabilities,
};

/// TypeScript editor protocol emitted for the frontend from Rust definitions.
pub fn generated_typescript_contract() -> String {
    let cfg = Config::default();
    let mut output =
        String::from("// Generated from Rust editor contract by ts-rs. Do not edit by hand.\n\n");
    output.push_str("export type U64String = string;\n\n");

    push_decl::<ScalarCellValue>(&mut output, &cfg);
    push_decl::<CellKind>(&mut output, &cfg);
    push_decl::<CellFormulaProjection>(&mut output, &cfg);
    push_decl::<CellData>(&mut output, &cfg);
    push_decl::<CellValue>(&mut output, &cfg);
    push_decl::<CellFormatProjection>(&mut output, &cfg);
    push_decl::<MergeRange>(&mut output, &cfg);
    push_decl::<SheetData>(&mut output, &cfg);
    push_decl::<SheetExtent>(&mut output, &cfg);
    push_decl::<FileData>(&mut output, &cfg);
    push_decl::<CellStyleProjection>(&mut output, &cfg);
    push_decl::<FreezePaneProjection>(&mut output, &cfg);
    push_decl::<HyperlinkProjection>(&mut output, &cfg);
    push_decl::<ReadOnlyRichProjection>(&mut output, &cfg);
    push_decl::<DrawingProjection>(&mut output, &cfg);
    push_decl::<DrawingKind>(&mut output, &cfg);
    push_decl::<SheetCapabilities>(&mut output, &cfg);
    push_decl::<WorkbookSaveCapabilities>(&mut output, &cfg);
    push_decl::<WorkbookStructureCapabilities>(&mut output, &cfg);
    push_decl::<WorkbookRichCapabilities>(&mut output, &cfg);
    push_decl::<WorkbookCapabilities>(&mut output, &cfg);
    push_decl::<DocumentCapabilities>(&mut output, &cfg);
    push_decl::<NativeSavePlan>(&mut output, &cfg);
    push_decl::<SpreadsheetFormatOptions>(&mut output, &cfg);
    push_decl::<crate::types::SheetCellChange>(&mut output, &cfg);
    push_decl::<SetCellRequest>(&mut output, &cfg);
    push_decl::<SearchResult>(&mut output, &cfg);
    push_decl::<SearchScope>(&mut output, &cfg);
    push_decl::<StorageType>(&mut output, &cfg);
    push_decl::<RecentFile>(&mut output, &cfg);
    push_decl::<AddRecentFileRequest>(&mut output, &cfg);
    push_decl::<HistoryStatus>(&mut output, &cfg);
    push_decl::<EditorStateInfo>(&mut output, &cfg);
    push_decl::<FormulaIssueKind>(&mut output, &cfg);
    push_decl::<FormulaIssue>(&mut output, &cfg);
    push_decl::<FormulaDiagnostics>(&mut output, &cfg);
    push_decl::<FormulaStatus>(&mut output, &cfg);
    push_decl::<LayoutPatch>(&mut output, &cfg);
    push_decl::<SheetInsertedPatch>(&mut output, &cfg);
    push_decl::<SheetDeletedPatch>(&mut output, &cfg);
    push_decl::<SheetUpdatedPatch>(&mut output, &cfg);
    push_decl::<SheetsReplacedPatch>(&mut output, &cfg);
    push_decl::<RichProjectionPatchScope>(&mut output, &cfg);
    push_decl::<RichProjectionPatch>(&mut output, &cfg);
    push_decl::<SheetStructureMetadataPatch>(&mut output, &cfg);
    push_decl::<RowInsertedPatch>(&mut output, &cfg);
    push_decl::<RowDeletedPatch>(&mut output, &cfg);
    push_decl::<ColumnInsertedPatch>(&mut output, &cfg);
    push_decl::<ColumnDeletedPatch>(&mut output, &cfg);
    push_decl::<SheetShapePatch>(&mut output, &cfg);
    push_decl::<ResyncRequiredPatch>(&mut output, &cfg);
    push_decl::<EditorPatch>(&mut output, &cfg);
    push_decl::<EditorCommandContext>(&mut output, &cfg);
    push_decl::<EditorMutationResponse>(&mut output, &cfg);
    push_decl::<EditorSessionInfo>(&mut output, &cfg);
    push_decl::<OpenDocumentResponse>(&mut output, &cfg);
    push_decl::<SheetProjectionResponse>(&mut output, &cfg);
    push_decl::<PreparedOpenDocument>(&mut output, &cfg);
    push_decl::<SavedDocumentIdentity>(&mut output, &cfg);
    push_decl::<SavedDocumentResponse>(&mut output, &cfg);
    push_tauri_command_map(&mut output);

    output
}

fn push_tauri_command_map(output: &mut String) {
    output.push_str(
        r#"export type TauriCommandMap = {
  "pick_open_file_desktop": { args: Record<string, never>, result: { path: string, fileName: string } | null },
  "discard_open_file_selection_desktop": { args: { path: string }, result: void },
  "prepare_open_file_desktop": { args: { path: string }, result: PreparedOpenDocument },
  "prepare_recent_file_desktop": { args: { id: string }, result: PreparedOpenDocument },
  "pick_save_location_desktop": { args: { defaultName: string }, result: string | null },
  "discard_save_location_desktop": { args: { path: string }, result: void },
  "save_file_desktop": { args: { path: string } & EditorCommandContext, result: SavedDocumentResponse },
  "export_file_desktop": { args: { defaultName: string } & EditorCommandContext, result: string | null },
  "pick_open_file_android": { args: Record<string, never>, result: { path: string, originalPath: string, fileName: string } | null },
  "discard_open_file_selection_android": { args: { path: string }, result: void },
  "prepare_open_file_android": { args: { path: string }, result: PreparedOpenDocument },
  "pick_save_location_android": { args: { defaultName: string }, result: string | null },
  "discard_save_location_android": { args: { path: string }, result: void },
  "save_file_android": { args: { path: string } & EditorCommandContext, result: SavedDocumentResponse },
  "export_file_android": { args: { defaultName: string } & EditorCommandContext, result: string | null },
  "pick_open_file_ios": { args: Record<string, never>, result: { path: string, originalPath: string, fileName: string } | null },
  "discard_open_file_selection_ios": { args: { path: string }, result: void },
  "prepare_open_file_ios": { args: { path: string }, result: PreparedOpenDocument },
  "pick_save_location_ios": { args: { defaultName: string }, result: string | null },
  "discard_save_location_ios": { args: { path: string }, result: void },
  "save_file_ios": { args: { path: string } & EditorCommandContext, result: SavedDocumentResponse },
  "export_file_ios": { args: { defaultName: string } & EditorCommandContext, result: string | null },
  "prepare_new_file": { args: { fileData: FileData }, result: PreparedOpenDocument },
  "commit_prepared_document": { args: { token: string, expectedDocumentId: U64String | null, expectedRevision: U64String | null }, result: OpenDocumentResponse },
  "abort_prepared_document": { args: { token: string }, result: void },
  "get_active_document": { args: Record<string, never>, result: OpenDocumentResponse | null },
  "get_current_file_data": { args: EditorCommandContext, result: FileData },
  "get_sheet_projection": { args: EditorCommandContext & { sheetIndex: number }, result: SheetProjectionResponse },
  "close_current_document": { args: { documentId: U64String }, result: void },
  "get_document_capabilities": { args: EditorCommandContext & { fileName: string, currentPath: string | null }, result: DocumentCapabilities },
  "get_native_save_plan": { args: EditorCommandContext & { targetPathOrName: string }, result: NativeSavePlan },
  "get_spreadsheet_format_options": { args: Record<string, never>, result: SpreadsheetFormatOptions },
  "get_editor_state": { args: { documentId: U64String | null, baseRevision: U64String | null }, result: EditorSessionInfo | null },
  "undo": { args: EditorCommandContext, result: EditorMutationResponse },
  "redo": { args: EditorCommandContext, result: EditorMutationResponse },
  "set_cell": { args: EditorCommandContext & { sheetIndex: number, row: number, col: number, text: string }, result: EditorMutationResponse },
  "set_cells": { args: EditorCommandContext & { changes: Array<SetCellRequest> }, result: EditorMutationResponse },
  "add_row": { args: EditorCommandContext & { sheetIndex: number, rowIndex: number }, result: EditorMutationResponse },
  "delete_row": { args: EditorCommandContext & { sheetIndex: number, rowIndex: number }, result: EditorMutationResponse },
  "add_column": { args: EditorCommandContext & { sheetIndex: number, colIndex: number }, result: EditorMutationResponse },
  "delete_column": { args: EditorCommandContext & { sheetIndex: number, colIndex: number }, result: EditorMutationResponse },
  "set_column_width": { args: EditorCommandContext & { sheetIndex: number, colIndex: number, width: number | null }, result: EditorMutationResponse },
  "set_row_height": { args: EditorCommandContext & { sheetIndex: number, rowIndex: number, height: number | null }, result: EditorMutationResponse },
  "add_sheet": { args: EditorCommandContext, result: EditorMutationResponse },
  "delete_sheet": { args: EditorCommandContext & { sheetIndex: number }, result: EditorMutationResponse },
  "search": { args: EditorCommandContext & { query: string, scope: SearchScope, currentSheetIndex: number | null }, result: Array<SearchResult> },
  "get_recent_files": { args: Record<string, never>, result: Array<RecentFile> },
  "add_recent_file_with_thumbnail": { args: { request: AddRecentFileRequest }, result: RecentFile },
  "remove_recent_file": { args: { id: string }, result: void },
  "check_update_mobile": { args: { currentVersion: string }, result: { version: string, tag_name: string, release_url: string, apk_url: string | null } | null },
}

"#,
    );
}

fn push_decl<T: TS + 'static>(output: &mut String, cfg: &Config) {
    output.push_str("export ");
    output.push_str(&T::decl(cfg));
    output.push_str("\n\n");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn generated_typescript_contract_is_current() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../src/types/generated.ts")
            .canonicalize()
            .expect("generated types path");
        let generated = generated_typescript_contract();
        if std::env::var_os("UPDATE_GENERATED_TYPES").is_some() {
            fs::write(&path, generated.as_bytes()).expect("write generated types");
        }

        let committed = fs::read_to_string(path).expect("read generated types");

        assert_eq!(committed, generated);
    }
}
