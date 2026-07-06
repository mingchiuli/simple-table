import { invoke } from "@tauri-apps/api/core";
import type {
  FileData,
  RecentFile,
  SetCellRequest,
  SearchResult,
  EditorMutationResponse,
  EditorSessionInfo,
  DocumentCapabilities,
  OpenDocumentResponse,
} from "@/types";

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

// ==================== Editor Operations ====================

export async function getEditorState(): Promise<EditorSessionInfo | null> {
  return invoke<EditorSessionInfo | null>("get_editor_state");
}

export async function undo(): Promise<EditorMutationResponse> {
  return invoke<EditorMutationResponse>("undo");
}

export async function redo(): Promise<EditorMutationResponse> {
  return invoke<EditorMutationResponse>("redo");
}

// ==================== Cell Operations ====================

export async function setCell(
  sheetIndex: number,
  row: number,
  col: number,
  text: string
): Promise<EditorMutationResponse> {
  return invoke<EditorMutationResponse>("set_cell", { sheetIndex, row, col, text });
}

export async function setCells(changes: SetCellRequest[]): Promise<EditorMutationResponse> {
  return invoke<EditorMutationResponse>("set_cells", { changes });
}

export async function addRow(sheetIndex: number, rowIndex: number): Promise<EditorMutationResponse> {
  return invoke<EditorMutationResponse>("add_row", { sheetIndex, rowIndex });
}

export async function deleteRow(sheetIndex: number, rowIndex: number): Promise<EditorMutationResponse> {
  return invoke<EditorMutationResponse>("delete_row", { sheetIndex, rowIndex });
}

export async function addColumn(sheetIndex: number, colIndex: number): Promise<EditorMutationResponse> {
  return invoke<EditorMutationResponse>("add_column", { sheetIndex, colIndex });
}

export async function deleteColumn(sheetIndex: number, colIndex: number): Promise<EditorMutationResponse> {
  return invoke<EditorMutationResponse>("delete_column", { sheetIndex, colIndex });
}

export async function setColumnWidth(
  sheetIndex: number,
  colIndex: number,
  width: number | null
): Promise<EditorMutationResponse> {
  return invoke<EditorMutationResponse>("set_column_width", { sheetIndex, colIndex, width });
}

export async function setRowHeight(
  sheetIndex: number,
  rowIndex: number,
  height: number | null
): Promise<EditorMutationResponse> {
  return invoke<EditorMutationResponse>("set_row_height", { sheetIndex, rowIndex, height });
}

// ==================== Sheet Operations ====================

export async function addSheet(): Promise<EditorMutationResponse> {
  return invoke<EditorMutationResponse>("add_sheet");
}

export async function deleteSheet(sheetIndex: number): Promise<EditorMutationResponse> {
  return invoke<EditorMutationResponse>("delete_sheet", { sheetIndex });
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
