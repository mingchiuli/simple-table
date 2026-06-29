import { invoke } from "@tauri-apps/api/core";
import type {
  FileData,
  RecentFile,
  CellValue,
  SearchResult,
  EditorMutationResponse,
  EditorStateInfo,
} from "@/types";

export async function initFile(fileData: FileData): Promise<void> {
  return invoke<void>("init_file", { fileData });
}

// ==================== Editor Operations ====================

export async function getEditorState(): Promise<EditorStateInfo | null> {
  return invoke<EditorStateInfo | null>("get_editor_state");
}

export async function markFileSaved(): Promise<void> {
  return invoke<void>("mark_file_saved");
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
  newValue: CellValue
): Promise<EditorMutationResponse> {
  return invoke<EditorMutationResponse>("set_cell", { sheetIndex, row, col, newValue });
}

export async function addRow(sheetIndex: number, rowIndex: number): Promise<EditorMutationResponse> {
  return invoke<EditorMutationResponse>("add_row", { sheetIndex, rowIndex });
}

export async function deleteRow(sheetIndex: number, rowIndex: number): Promise<EditorMutationResponse> {
  return invoke<EditorMutationResponse>("delete_row", { sheetIndex, rowIndex });
}

export async function addColumn(sheetIndex: number): Promise<EditorMutationResponse> {
  return invoke<EditorMutationResponse>("add_column", { sheetIndex });
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
  bytes: number[],
  storageType?: 'mobileSandboxPath' | 'desktopPath',
  originalPath?: string
): Promise<RecentFile> {
  return invoke<RecentFile>("add_recent_file_with_thumbnail", {
    request: {
      path,
      fileName,
      fileSize,
      bytes,
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

export async function generateCurrentThumbnailBytes(): Promise<number[]> {
  try {
    return await invoke<number[]>("generate_current_thumbnail_bytes");
  } catch (error) {
    console.warn("Failed to generate current thumbnail bytes:", error);
    return [];
  }
}
