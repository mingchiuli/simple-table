export type ScalarCellValue = string | number | boolean | null;

export type FormulaCellValue = {
  type: 'formula';
  formula: string;
  cachedValue: CellValue;
  error?: string;
};

export type CellValue = ScalarCellValue | FormulaCellValue;

export interface MergeRange {
  startRow: number;
  startCol: number;
  endRow: number;
  endCol: number;
}

export interface SheetData {
  name: string;
  rows: CellValue[][];
  merges: MergeRange[];
  columnWidths?: Record<number, number>;
  rowHeights?: Record<number, number>;
}

export interface FileData {
  path: string;
  fileName: string;
  sheets: SheetData[];
}

export interface CellChange {
  row: number;
  col: number;
  value: CellValue;
}

export interface SheetCellChange {
  sheetIndex: number;
  row: number;
  col: number;
  value: CellValue;
}

export interface RowChange {
  index: number;
  values: CellValue[];
}

export interface ColumnChange {
  index: number;
}

export interface ColumnWidthChange {
  colIndex: number;
  width?: number | null;
}

export interface RowHeightChange {
  rowIndex: number;
  height?: number | null;
}

// Rust 使用 #[serde(tag = "type", content = "data")]，所以格式是 { type: 'SetCell', data: {...} }
export type OperationResult =
  | { type: 'SetCell'; data: { sheetIndex: number; cell: CellChange } }
  | { type: 'AddRow'; data: { sheetIndex: number; row: RowChange } }
  | { type: 'DeleteRow'; data: { sheetIndex: number; rowIndex: number } }
  | { type: 'AddColumn'; data: { sheetIndex: number; column: ColumnChange; colData: CellValue[] } }
  | { type: 'DeleteColumn'; data: { sheetIndex: number; columnIndex: number } }
  | { type: 'SetColumnWidth'; data: { sheetIndex: number; column: ColumnWidthChange } }
  | { type: 'SetRowHeight'; data: { sheetIndex: number; row: RowHeightChange } }
  | { type: 'AddSheet'; data: { sheetIndex: number; name: string; sheetData: SheetData } }
  | { type: 'DeleteSheet'; data: { sheetIndex: number; sheetData: SheetData } };

export interface SearchResult {
  sheetIndex: number;
  sheetName: string;
  row: number;
  col: number;
  value: string;
  cellPosition: string;
}

export interface RecentFile {
  id: string;
  path: string;
  fileName: string;
  lastOpened: number;
  fileSize: number;
  thumbnail?: string;
  storageType?: 'mobileSandboxPath' | 'desktopPath';
  originalPath?: string;
}

export interface EditorStateInfo {
  canUndo: boolean;
  canRedo: boolean;
  isDirty: boolean;
}

export interface LayoutPatch {
  sheetIndex: number;
  columnWidths?: Record<number, number | null>;
  rowHeights?: Record<number, number | null>;
}

export type EditorPatch =
  | { type: 'Cells'; data: { changes: SheetCellChange[] } }
  | { type: 'Layout'; data: { patch: LayoutPatch } }
  | { type: 'FullSnapshot'; data: { fileData: FileData } };

export interface EditorMutationResponse {
  editorState: EditorStateInfo;
  patches?: EditorPatch[];
}
