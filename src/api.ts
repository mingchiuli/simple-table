import { invokeCommand } from "@/tauriInvoke";
import type {
  FileData,
  RecentFile,
  SetCellRequest,
  SearchResult,
  EditorMutationResponse,
  EditorSessionInfo,
  DocumentCapabilities,
  NativeSavePlan,
  OpenDocumentResponse,
  PreparedOpenDocument,
  SpreadsheetFormatOptions,
  EditorCommandContext,
  SearchScope,
  AddRecentFileRequest,
  SheetProjectionResponse,
  U64String,
} from "@/types";

export async function prepareNewFile(fileData: FileData): Promise<PreparedOpenDocument> {
  return invokeCommand("prepare_new_file", { fileData });
}

export async function commitPreparedDocument(
  token: string,
  expectedContext: EditorCommandContext | null
): Promise<OpenDocumentResponse> {
  return invokeCommand("commit_prepared_document", {
    token,
    expectedDocumentId: expectedContext?.documentId ?? null,
    expectedRevision: expectedContext?.baseRevision ?? null,
  });
}

export async function abortPreparedDocument(token: string): Promise<void> {
  return invokeCommand("abort_prepared_document", { token });
}

export async function getActiveDocument(): Promise<OpenDocumentResponse | null> {
  return invokeCommand("get_active_document", {});
}

export async function getCurrentFileData(context: EditorCommandContext): Promise<FileData> {
  return invokeCommand("get_current_file_data", context);
}

export async function getSheetProjection(
  context: EditorCommandContext,
  sheetIndex: number
): Promise<SheetProjectionResponse> {
  return invokeCommand("get_sheet_projection", { ...context, sheetIndex });
}

export async function closeCurrentDocument(documentId: U64String): Promise<void> {
  return invokeCommand("close_current_document", { documentId });
}

export async function getDocumentCapabilities(
  context: EditorCommandContext,
  fileName: string,
  currentPath: string | null
): Promise<DocumentCapabilities> {
  return invokeCommand("get_document_capabilities", {
    ...context,
    fileName,
    currentPath,
  });
}

export async function getNativeSavePlan(
  context: EditorCommandContext,
  targetPathOrName: string
): Promise<NativeSavePlan> {
  return invokeCommand("get_native_save_plan", { ...context, targetPathOrName });
}

export async function getSpreadsheetFormatOptions(): Promise<SpreadsheetFormatOptions> {
  return invokeCommand("get_spreadsheet_format_options", {});
}

// ==================== Editor Operations ====================

export async function getEditorState(
  context: EditorCommandContext | null = null
): Promise<EditorSessionInfo | null> {
  return invokeCommand("get_editor_state", {
    documentId: context?.documentId ?? null,
    baseRevision: context?.baseRevision ?? null,
  });
}

export async function undo(context: EditorCommandContext): Promise<EditorMutationResponse> {
  return invokeCommand("undo", context);
}

export async function redo(context: EditorCommandContext): Promise<EditorMutationResponse> {
  return invokeCommand("redo", context);
}

// ==================== Cell Operations ====================

export async function setCell(
  context: EditorCommandContext,
  sheetIndex: number,
  row: number,
  col: number,
  text: string
): Promise<EditorMutationResponse> {
  return invokeCommand("set_cell", { ...context, sheetIndex, row, col, text });
}

export async function setCells(
  context: EditorCommandContext,
  changes: SetCellRequest[]
): Promise<EditorMutationResponse> {
  return invokeCommand("set_cells", { ...context, changes });
}

export async function addRow(
  context: EditorCommandContext,
  sheetIndex: number,
  rowIndex: number
): Promise<EditorMutationResponse> {
  return invokeCommand("add_row", { ...context, sheetIndex, rowIndex });
}

export async function deleteRow(
  context: EditorCommandContext,
  sheetIndex: number,
  rowIndex: number
): Promise<EditorMutationResponse> {
  return invokeCommand("delete_row", { ...context, sheetIndex, rowIndex });
}

export async function addColumn(
  context: EditorCommandContext,
  sheetIndex: number,
  colIndex: number
): Promise<EditorMutationResponse> {
  return invokeCommand("add_column", { ...context, sheetIndex, colIndex });
}

export async function deleteColumn(
  context: EditorCommandContext,
  sheetIndex: number,
  colIndex: number
): Promise<EditorMutationResponse> {
  return invokeCommand("delete_column", { ...context, sheetIndex, colIndex });
}

export async function setColumnWidth(
  context: EditorCommandContext,
  sheetIndex: number,
  colIndex: number,
  width: number | null
): Promise<EditorMutationResponse> {
  return invokeCommand("set_column_width", {
    ...context,
    sheetIndex,
    colIndex,
    width,
  });
}

export async function setRowHeight(
  context: EditorCommandContext,
  sheetIndex: number,
  rowIndex: number,
  height: number | null
): Promise<EditorMutationResponse> {
  return invokeCommand("set_row_height", {
    ...context,
    sheetIndex,
    rowIndex,
    height,
  });
}

// ==================== Sheet Operations ====================

export async function addSheet(context: EditorCommandContext): Promise<EditorMutationResponse> {
  return invokeCommand("add_sheet", context);
}

export async function deleteSheet(
  context: EditorCommandContext,
  sheetIndex: number
): Promise<EditorMutationResponse> {
  return invokeCommand("delete_sheet", { ...context, sheetIndex });
}

// ==================== Search Operations ====================

export async function search(
  context: EditorCommandContext,
  query: string,
  scope: SearchScope,
  currentSheetIndex: number | null
): Promise<SearchResult[]> {
  return invokeCommand("search", { ...context, query, scope, currentSheetIndex });
}

// ==================== Recent Files Operations ====================

export async function getRecentFiles(): Promise<RecentFile[]> {
  return invokeCommand("get_recent_files", {});
}

export async function addRecentFileWithThumbnail(
  context: EditorCommandContext,
  originalPath?: string
): Promise<RecentFile> {
  const request: AddRecentFileRequest = {
    originalPath,
    documentId: context.documentId,
    baseRevision: context.baseRevision,
  };

  return invokeCommand("add_recent_file_with_thumbnail", {
    request,
  });
}

export async function removeRecentFile(id: string): Promise<void> {
  return invokeCommand("remove_recent_file", { id });
}
