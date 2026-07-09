import { invoke } from "@tauri-apps/api/core";
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
  SpreadsheetFormatOptions,
  EditorCommandContext,
  StorageType,
  SearchScope,
  AddRecentFileRequest,
} from "@/types";

export async function initFile(fileData: FileData): Promise<OpenDocumentResponse> {
  return invoke<OpenDocumentResponse>("init_file", { fileData });
}

export async function getCurrentFileData(context: EditorCommandContext): Promise<FileData> {
  return invoke<FileData>("get_current_file_data", context);
}

export async function closeCurrentDocument(documentId: number): Promise<void> {
  return invoke<void>("close_current_document", { documentId });
}

export async function getDocumentCapabilities(
  context: EditorCommandContext,
  fileName: string,
  currentPath: string | null
): Promise<DocumentCapabilities> {
  return invoke<DocumentCapabilities>("get_document_capabilities", {
    ...context,
    fileName,
    currentPath,
  });
}

export async function getNativeSavePlan(
  context: EditorCommandContext,
  targetPathOrName: string
): Promise<NativeSavePlan> {
  return invoke<NativeSavePlan>("get_native_save_plan", { ...context, targetPathOrName });
}

export async function getSpreadsheetFormatOptions(): Promise<SpreadsheetFormatOptions> {
  return invoke<SpreadsheetFormatOptions>("get_spreadsheet_format_options");
}

// ==================== Editor Operations ====================

export async function getEditorState(
  context: EditorCommandContext | null = null
): Promise<EditorSessionInfo | null> {
  return invoke<EditorSessionInfo | null>("get_editor_state", {
    documentId: context?.documentId ?? null,
    baseRevision: context?.baseRevision ?? null,
  });
}

export async function undo(context: EditorCommandContext): Promise<EditorMutationResponse> {
  return invoke<EditorMutationResponse>("undo", context);
}

export async function redo(context: EditorCommandContext): Promise<EditorMutationResponse> {
  return invoke<EditorMutationResponse>("redo", context);
}

// ==================== Cell Operations ====================

export async function setCell(
  context: EditorCommandContext,
  sheetIndex: number,
  row: number,
  col: number,
  text: string
): Promise<EditorMutationResponse> {
  return invoke<EditorMutationResponse>("set_cell", { ...context, sheetIndex, row, col, text });
}

export async function setCells(
  context: EditorCommandContext,
  changes: SetCellRequest[]
): Promise<EditorMutationResponse> {
  return invoke<EditorMutationResponse>("set_cells", { ...context, changes });
}

export async function addRow(
  context: EditorCommandContext,
  sheetIndex: number,
  rowIndex: number
): Promise<EditorMutationResponse> {
  return invoke<EditorMutationResponse>("add_row", { ...context, sheetIndex, rowIndex });
}

export async function deleteRow(
  context: EditorCommandContext,
  sheetIndex: number,
  rowIndex: number
): Promise<EditorMutationResponse> {
  return invoke<EditorMutationResponse>("delete_row", { ...context, sheetIndex, rowIndex });
}

export async function addColumn(
  context: EditorCommandContext,
  sheetIndex: number,
  colIndex: number
): Promise<EditorMutationResponse> {
  return invoke<EditorMutationResponse>("add_column", { ...context, sheetIndex, colIndex });
}

export async function deleteColumn(
  context: EditorCommandContext,
  sheetIndex: number,
  colIndex: number
): Promise<EditorMutationResponse> {
  return invoke<EditorMutationResponse>("delete_column", { ...context, sheetIndex, colIndex });
}

export async function setColumnWidth(
  context: EditorCommandContext,
  sheetIndex: number,
  colIndex: number,
  width: number | null
): Promise<EditorMutationResponse> {
  return invoke<EditorMutationResponse>("set_column_width", {
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
  return invoke<EditorMutationResponse>("set_row_height", {
    ...context,
    sheetIndex,
    rowIndex,
    height,
  });
}

// ==================== Sheet Operations ====================

export async function addSheet(context: EditorCommandContext): Promise<EditorMutationResponse> {
  return invoke<EditorMutationResponse>("add_sheet", context);
}

export async function deleteSheet(
  context: EditorCommandContext,
  sheetIndex: number
): Promise<EditorMutationResponse> {
  return invoke<EditorMutationResponse>("delete_sheet", { ...context, sheetIndex });
}

// ==================== Search Operations ====================

export async function search(
  context: EditorCommandContext,
  query: string,
  scope: SearchScope,
  currentSheetIndex: number | null
): Promise<SearchResult[]> {
  return invoke<SearchResult[]>("search", { ...context, query, scope, currentSheetIndex });
}

// ==================== Recent Files Operations ====================

export async function getRecentFiles(): Promise<RecentFile[]> {
  return invoke<RecentFile[]>("get_recent_files");
}

export async function addRecentFileWithThumbnail(
  path: string,
  fileName: string,
  fileSize: number,
  storageType?: StorageType,
  originalPath?: string,
  context?: EditorCommandContext | null
): Promise<RecentFile> {
  const request: AddRecentFileRequest = {
    path,
    fileName,
    fileSize,
    storageType,
    originalPath,
    ...(context
      ? {
        documentId: context.documentId,
        baseRevision: context.baseRevision,
      }
      : {}),
  };

  return invoke<RecentFile>("add_recent_file_with_thumbnail", {
    request,
  });
}

export async function removeRecentFile(id: string): Promise<void> {
  return invoke<void>("remove_recent_file", { id });
}

export async function checkFileExists(path: string): Promise<boolean> {
  return invoke<boolean>("check_file_exists", { path });
}

export async function getFileSize(path: string): Promise<number> {
  return invoke<number>("get_file_size", { path });
}

export async function updateRecentFilePath(id: string, newPath: string): Promise<void> {
  return invoke<void>("update_recent_file_path", { id, newPath });
}
