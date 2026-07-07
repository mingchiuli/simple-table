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
} from "@/types";

export type EditorCommandContext = {
  documentId: number;
  baseRevision: number;
};

export async function initFile(fileData: FileData): Promise<OpenDocumentResponse> {
  return invoke<OpenDocumentResponse>("init_file", { fileData });
}

export async function getCurrentFileData(): Promise<FileData> {
  return invoke<FileData>("get_current_file_data");
}

export async function updateDocumentIdentity(path: string, fileName: string): Promise<void> {
  return invoke<void>("update_document_identity", { path, fileName });
}

export async function getDocumentCapabilities(
  fileName: string,
  currentPath: string | null
): Promise<DocumentCapabilities> {
  return invoke<DocumentCapabilities>("get_document_capabilities", { fileName, currentPath });
}

export async function getNativeSavePlan(targetPathOrName: string): Promise<NativeSavePlan> {
  return invoke<NativeSavePlan>("get_native_save_plan", { targetPathOrName });
}

// ==================== Editor Operations ====================

export async function getEditorState(): Promise<EditorSessionInfo | null> {
  return invoke<EditorSessionInfo | null>("get_editor_state");
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
  query: string,
  scope: "currentSheet" | "allSheets",
  currentSheetIndex: number | null
): Promise<SearchResult[]> {
  return invoke<SearchResult[]>("search", { query, scope, currentSheetIndex });
}

// ==================== Recent Files Operations ====================

export async function getRecentFiles(): Promise<RecentFile[]> {
  return invoke<RecentFile[]>("get_recent_files");
}

export async function addRecentFileWithThumbnail(
  path: string,
  fileName: string,
  fileSize: number,
  storageType?: 'mobileSandboxPath' | 'desktopPath',
  originalPath?: string
): Promise<RecentFile> {
  return invoke<RecentFile>("add_recent_file_with_thumbnail", {
    request: {
      path,
      fileName,
      fileSize,
      storageType,
      originalPath,
    },
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
