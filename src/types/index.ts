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

export interface DocumentCapabilities {
  nativeSaveExtension: 'xlsx' | null;
  exportExtension: 'xlsx' | 'csv';
  requiresSaveAsForNativeSave: boolean;
}

export interface SheetCellChange {
  sheetIndex: number;
  row: number;
  col: number;
  value: CellValue;
}

export interface SetCellRequest {
  sheetIndex: number;
  row: number;
  col: number;
  text: string;
}

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

export type FormulaStatus =
  | { state: 'ready' }
  | { state: 'degraded'; message: string };

export interface LayoutPatch {
  sheetIndex: number;
  columnWidths?: Record<number, number | null>;
  rowHeights?: Record<number, number | null>;
}

export type EditorPatch =
  | { type: 'Cells'; data: { changes: SheetCellChange[] } }
  | { type: 'Layout'; data: { patch: LayoutPatch } }
  | { type: 'SheetSnapshot'; data: { sheetIndex: number; sheet: SheetData } }
  | { type: 'FullSnapshot'; data: { fileData: FileData } };

export interface EditorMutationResponse {
  protocolVersion: 1;
  documentId: number;
  revision: number;
  formulaStatus: FormulaStatus;
  editorState: EditorStateInfo;
  patches?: EditorPatch[];
}

export interface EditorSessionInfo {
  documentId: number;
  revision: number;
  formulaStatus: FormulaStatus;
  editorState: EditorStateInfo;
}
