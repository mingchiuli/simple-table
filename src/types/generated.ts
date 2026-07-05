// Generated from Rust editor contract. Do not edit by hand.

export type ScalarCellValue = string | number | boolean | null;

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

export type CellValue = CellData;

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
  cellFormats?: Record<string, CellFormatProjection>;
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
  blockedEditReasons?: string[];
  blockedResizeReasons?: string[];
  blockedRowStructureReasons?: string[];
  blockedColumnStructureReasons?: string[];
  blockedSheetStructureReasons?: string[];
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
  skippedReferenceRewriteCount: number;
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

export interface SheetUpdatedPatch {
  sheetIndex: number;
  sheet: SheetData;
}

export interface SheetMetadataPatch {
  sheetIndex: number;
  merges: MergeRange[];
  columnWidths: Record<number, number>;
  rowHeights: Record<number, number>;
  rich: SheetRichProjection;
}

export interface RowsInsertedPatch {
  sheetIndex: number;
  rowIndex: number;
  rows: CellValue[][];
}

export interface RowsDeletedPatch {
  sheetIndex: number;
  rowIndex: number;
  count: number;
}

export interface ColumnsInsertedPatch {
  sheetIndex: number;
  colIndex: number;
  values: CellValue[];
}

export interface ColumnsDeletedPatch {
  sheetIndex: number;
  colIndex: number;
  count: number;
}

export interface SheetShapePatch {
  sheetIndex: number;
  rowLengths: number[];
}

export interface ResyncRequiredPatch {
  reason: string;
}

export type EditorPatch =
  | { type: 'Cells'; data: { changes: SheetCellChange[] } }
  | { type: 'Layout'; data: { patch: LayoutPatch } }
  | { type: 'SheetInserted'; data: { patch: SheetInsertedPatch } }
  | { type: 'SheetDeleted'; data: { patch: SheetDeletedPatch } }
  | { type: 'SheetUpdated'; data: { patch: SheetUpdatedPatch } }
  | { type: 'SheetMetadata'; data: { patch: SheetMetadataPatch } }
  | { type: 'RowsInserted'; data: { patch: RowsInsertedPatch } }
  | { type: 'RowsDeleted'; data: { patch: RowsDeletedPatch } }
  | { type: 'ColumnsInserted'; data: { patch: ColumnsInsertedPatch } }
  | { type: 'ColumnsDeleted'; data: { patch: ColumnsDeletedPatch } }
  | { type: 'SheetShape'; data: { patch: SheetShapePatch } }
  | { type: 'ResyncRequired'; data: { patch: ResyncRequiredPatch } };

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

export interface OpenDocumentResponse {
  fileData: FileData;
  editorSession: EditorSessionInfo;
}
