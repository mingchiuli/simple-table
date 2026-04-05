import { invoke } from "@tauri-apps/api/core";
import type { FileData, RecentFile, CellValue, OperationResult, SearchResult, SortState } from "@/types";

// ==================== File Operations ====================

export async function readFileBytes(path: string, bytes: number[], fileName?: string): Promise<FileData> {
  return invoke<FileData>("read_file_bytes", { path, bytes, fileName });
}

export async function generateFileBytes(fileData: FileData): Promise<[string, number[]]> {
  return invoke<[string, number[]]>("generate_file_bytes", { fileData });
}

export async function initFile(fileData: FileData): Promise<void> {
  return invoke<void>("init_file", { fileData });
}

// ==================== Editor Operations ====================

export async function getEditorState(): Promise<{ canUndo: boolean; canRedo: boolean }> {
  return invoke<{ canUndo: boolean; canRedo: boolean }>("get_editor_state");
}

export async function undo(): Promise<OperationResult> {
  return invoke<OperationResult>("undo");
}

export async function redo(): Promise<OperationResult> {
  return invoke<OperationResult>("redo");
}

// ==================== Cell Operations ====================

export async function setCell(
  sheetIndex: number,
  row: number,
  col: number,
  oldValue: CellValue,
  newValue: CellValue
): Promise<void> {
  return invoke<void>("set_cell", { sheetIndex, row, col, oldValue, newValue });
}

export async function addRow(sheetIndex: number, rowIndex: number): Promise<void> {
  return invoke<void>("add_row", { sheetIndex, rowIndex });
}

export async function deleteRow(sheetIndex: number, rowIndex: number): Promise<void> {
  return invoke<void>("delete_row", { sheetIndex, rowIndex });
}

export async function addColumn(sheetIndex: number): Promise<void> {
  return invoke<void>("add_column", { sheetIndex });
}

export async function deleteColumn(sheetIndex: number, colIndex: number): Promise<void> {
  return invoke<void>("delete_column", { sheetIndex, colIndex });
}

// ==================== Sheet Operations ====================

export async function addSheet(): Promise<void> {
  return invoke<void>("add_sheet");
}

export async function deleteSheet(sheetIndex: number): Promise<void> {
  return invoke<void>("delete_sheet", { sheetIndex });
}

// ==================== Sort Operations ====================

export async function sortColumn(
  sheetIndex: number,
  colIndex: number,
  ascending: boolean,
  previousSortState: SortState | null
): Promise<OperationResult> {
  return invoke<OperationResult>("sort_column", {
    sheetIndex,
    colIndex,
    ascending,
    previousSortState,
  });
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
  extension: string,
  storageType?: 'androidUri' | 'iosPrivate' | 'desktopPath',
  originalPath?: string
): Promise<RecentFile> {
  return invoke<RecentFile>("add_recent_file_with_thumbnail", {
    path,
    fileName,
    fileSize,
    bytes,
    extension,
    storageType,
    originalPath,
  });
}

export async function removeRecentFile(id: string): Promise<void> {
  return invoke<void>("remove_recent_file", { id });
}

export async function checkFileExists(path: string): Promise<boolean> {
  return invoke<boolean>("check_file_exists", { path });
}

export async function updateRecentFilePath(id: string, newPath: string): Promise<void> {
  return invoke<void>("update_recent_file_path", { id, newPath });
}

// ==================== Android File Operations ====================

export interface PickedFile {
  path: string;
  fileName: string;
  bytes: number[];
}

/**
 * Android: 打开文件选择器并持久化 URI 权限
 */
export async function pickFileAndroid(): Promise<PickedFile> {
  return invoke<PickedFile>("pick_file_android");
}

/**
 * Android: 从持久化 URI 读取文件
 */
export async function readFileAndroid(uri: string): Promise<number[]> {
  return invoke<number[]>("read_file_android", { uri });
}

/**
 * Android: 保存文件到指定 URI
 */
export async function saveFileAndroid(uri: string, bytes: number[]): Promise<void> {
  return invoke<void>("save_file_android", { uri, bytes });
}

/**
 * Android: 选择保存位置
 */
export async function pickSaveLocationAndroid(defaultName: string): Promise<string | null> {
  return invoke<string | null>("pick_save_location_android", { defaultName });
}
