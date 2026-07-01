export type ScalarCellValue = string | number | boolean | null;

export type FormulaCellValue = {
  type: 'formula';
  formula: string;
  cachedValue: CellValue;
  error?: string;
};

export interface CellFormatProjection {
  numberFormat?: string;
  styleId?: string;
}

export type CellKind = 'blank' | 'text' | 'number' | 'boolean' | 'formula' | 'error';

export interface CellFormulaProjection {
  formula: string;
  cachedValue: CellValue;
  error?: string;
}

export interface CellData {
  type: 'cell';
  kind: CellKind;
  raw: ScalarCellValue;
  display: string;
  formula?: CellFormulaProjection;
  format?: CellFormatProjection;
}

export type CellValue = ScalarCellValue | FormulaCellValue | CellData;

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
  rich?: SheetRichProjection;
}

export interface FileData {
  path: string;
  fileName: string;
  sheets: SheetData[];
}

export interface SheetRichProjection {
  cellStyles?: Record<string, CellStyleProjection>;
  drawings?: DrawingProjection[];
  hasMoreDrawings?: boolean;
}

export interface CellStyleProjection {
  fontColor?: string;
  backgroundColor?: string;
  bold?: boolean;
  italic?: boolean;
  horizontalAlign?: string;
  verticalAlign?: string;
  numberFormat?: string;
}

export interface DrawingProjection {
  kind: DrawingKind;
  fromRow: number;
  fromCol: number;
  toRow?: number;
  toCol?: number;
}

export type DrawingKind = 'image' | 'chart';

export interface WorkbookCapabilities {
  canEditCells: boolean;
  canResizeRowsColumns: boolean;
  canInsertDeleteRows: boolean;
  canInsertDeleteColumns: boolean;
  canInsertDeleteSheets: boolean;
  canNativeSave: boolean;
  blockedStructureReasons?: string[];
  detectedFeatures?: string[];
}

export interface DocumentCapabilities {
  nativeSaveExtension: 'xlsx' | null;
  exportExtension: 'xlsx' | 'csv';
  requiresSaveAsForNativeSave: boolean;
  workbook: WorkbookCapabilities;
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

export interface FormulaDiagnostics {
  invalidFormulaCount: number;
  volatileFormulaCount: number;
  unsupportedDependencyCount: number;
  largeRangeDependencyCount: number;
}

export type FormulaStatus =
  | { state: 'ready'; diagnostics?: FormulaDiagnostics }
  | { state: 'degraded'; message: string; diagnostics?: FormulaDiagnostics };

export interface LayoutPatch {
  sheetIndex: number;
  columnWidths?: Record<number, number | null>;
  rowHeights?: Record<number, number | null>;
}

export interface SheetInsertedPatch {
  sheetIndex: number;
  sheet: SheetData;
}

export interface SheetDeletedPatch {
  sheetIndex: number;
}

export type EditorPatch =
  | { type: 'Cells'; data: { changes: SheetCellChange[] } }
  | { type: 'Layout'; data: { patch: LayoutPatch } }
  | { type: 'SheetInserted'; data: { patch: SheetInsertedPatch } }
  | { type: 'SheetDeleted'; data: { patch: SheetDeletedPatch } }
  | { type: 'SheetSnapshot'; data: { sheetIndex: number; sheet: SheetData } }
  | { type: 'FullSnapshot'; data: { fileData: FileData } };

export interface EditorMutationResponse {
  protocolVersion: 1;
  documentId: number;
  revision: number;
  formulaStatus: FormulaStatus;
  capabilities: WorkbookCapabilities;
  editorState: EditorStateInfo;
  patches?: EditorPatch[];
}

export interface EditorSessionInfo {
  documentId: number;
  revision: number;
  formulaStatus: FormulaStatus;
  capabilities: WorkbookCapabilities;
  editorState: EditorStateInfo;
}

export function defaultWorkbookCapabilities(): WorkbookCapabilities {
  return {
    canEditCells: true,
    canResizeRowsColumns: true,
    canInsertDeleteRows: true,
    canInsertDeleteColumns: true,
    canInsertDeleteSheets: true,
    canNativeSave: true,
    blockedStructureReasons: [],
    detectedFeatures: [],
  };
}
