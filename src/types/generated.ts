// Generated from Rust editor contract by ts-rs. Do not edit by hand.

export type ScalarCellValue = string | number | boolean | null;

export type CellKind = "blank" | "text" | "number" | "boolean" | "formula" | "error";

export type CellFormulaProjection = { formula: string, cachedValue: CellValue, error?: string, };

export type CellData = { type: "cell", kind: CellKind, raw: ScalarCellValue, display: string, formula?: CellFormulaProjection, format?: CellFormatProjection, };

export type CellValue = CellData;

export type CellFormatProjection = { numberFormat?: string | null, styleId?: string | null, };

export type MergeRange = { startRow: number, startCol: number, endRow: number, endCol: number, };

export type SheetData = { name: string, rows: Array<Array<CellValue>>, 
/**
 * 合并范围
 */
merges: Array<MergeRange>, 
/**
 * 列宽配置（用于持久化）
 */
columnWidths?: { [key in number]: number }, 
/**
 * 行高配置（持久化到 Excel，属于文档状态）
 */
rowHeights?: { [key in number]: number }, 
/**
 * Read-only rich Excel projection. This is display metadata only; the
 * original workbook remains the persistence source for styles and drawings.
 */
rich: ReadOnlyRichProjection, };

export type FileData = { path: string, fileName: string, sheets: Array<SheetData>, };

export type CellStyleProjection = { fontColor?: string | null, backgroundColor?: string | null, bold?: boolean | null, italic?: boolean | null, horizontalAlign?: string | null, verticalAlign?: string | null, numberFormat?: string | null, };

export type FreezePaneProjection = { topLeftCell: string, horizontalSplit: number, verticalSplit: number, activePane: string, state: string, };

export type HyperlinkProjection = { url: string, tooltip?: string | null, location: boolean, };

export type ReadOnlyRichProjection = { cellFormats?: { [key in string]: CellFormatProjection }, cellStyles?: { [key in string]: CellStyleProjection }, hiddenRows?: Array<number>, hiddenColumns?: Array<number>, freezePane?: FreezePaneProjection | null, hyperlinks?: { [key in string]: HyperlinkProjection }, drawings?: Array<DrawingProjection>, hasMoreDrawings: boolean, hasStyleMetadata: boolean, hasHyperlinks: boolean, hasFreezePane: boolean, };

export type DrawingProjection = { kind: DrawingKind, fromRow: number, fromCol: number, toRow?: number | null, toCol?: number | null, };

export type DrawingKind = "image" | "chart";

export type SheetCapabilities = { canEditCells: boolean, canResizeRowsColumns: boolean, canInsertDeleteRows: boolean, canInsertDeleteColumns: boolean, blockedEditReasons?: Array<string>, blockedResizeReasons?: Array<string>, blockedRowStructureReasons?: Array<string>, blockedColumnStructureReasons?: Array<string>, };

export type WorkbookSaveCapabilities = { canNativeSave: boolean, blockedSaveReasons?: Array<string>, detectedFeatures?: Array<string>, };

export type WorkbookStructureCapabilities = { canInsertDeleteSheets: boolean, blockedStructureReasons?: Array<string>, blockedSheetStructureReasons?: Array<string>, };

export type WorkbookRichCapabilities = { canEditStyles: boolean, canEditDrawings: boolean, canEditHyperlinks: boolean, };

export type WorkbookCapabilities = { save: WorkbookSaveCapabilities, structure: WorkbookStructureCapabilities, rich: WorkbookRichCapabilities, sheets?: Array<SheetCapabilities>, };

export type DocumentCapabilities = { sourceFormat: "xlsx" | "csv", canSaveOriginal: boolean, nativeSaveFormat: "xlsx" | "csv" | null, exportFormats: Array<"xlsx" | "csv">, nativeSaveExtension: "xlsx" | "csv" | null, exportExtension: "xlsx" | "csv", requiresSaveAsForNativeSave: boolean, workbook: WorkbookCapabilities, };

export type NativeSavePlan = { canSave: boolean, requiresSaveAs: boolean, nativeSaveExtension: "xlsx" | "csv" | null, defaultExtension: "xlsx" | "csv", blockedReason?: string | null, capabilities: DocumentCapabilities, };

export type SheetCellChange = { sheetIndex: number, row: number, col: number, value: CellValue, display?: string, format?: CellFormatProjection, style?: CellStyleProjection, };

export type SetCellRequest = { sheetIndex: number, row: number, col: number, text: string, };

export type SearchResult = { sheetIndex: number, sheetName: string, row: number, col: number, value: string, cellPosition: string, };

export type SearchScope = "currentSheet" | "allSheets";

export type StorageType = "mobileSandboxPath" | "desktopPath";

export type RecentFile = { id: string, path: string, fileName: string, lastOpened: number, fileSize: number, thumbnail?: string, 
/**
 * 存储类型（用于区分不同平台的文件处理方式）
 */
storageType: StorageType, 
/**
 * iOS: 原始文件路径（用于显示）
 */
originalPath?: string, };

export type HistoryStatus = { isTruncated: boolean, reason?: string, undoEntries: number, redoEntries: number, undoEstimatedBytes: number, redoEstimatedBytes: number, maxHistoryBytes: number, maxSingleEntryBytes: number, };

export type EditorStateInfo = { canUndo: boolean, canRedo: boolean, isDirty: boolean, history: HistoryStatus, };

export type FormulaIssueKind = "invalidFormula" | "volatileFormula" | "unsupportedDependency" | "largeRangeDependency";

export type FormulaIssue = { sheetIndex: number, row: number, col: number, kind: FormulaIssueKind, message: string, };

export type FormulaDiagnostics = { invalidFormulaCount: number, volatileFormulaCount: number, unsupportedDependencyCount: number, largeRangeDependencyCount: number, skippedReferenceRewriteCount: number, issues?: Array<FormulaIssue>, };

export type FormulaStatus = { "state": "ready", diagnostics: FormulaDiagnostics, } | { "state": "degraded", message: string, diagnostics: FormulaDiagnostics, };

export type LayoutPatch = { sheetIndex: number, columnWidths?: { [key in number]: number | null }, rowHeights?: { [key in number]: number | null }, };

export type SheetInsertedPatch = { sheetIndex: number, sheet: SheetData, };

export type SheetDeletedPatch = { sheetIndex: number, };

export type SheetUpdatedPatch = { sheetIndex: number, sheet: SheetData, };

export type SheetsReplacedPatch = { startIndex: number, sheets: Array<SheetData>, };

export type RichProjectionPatchScope = { "type": "all" } | { "type": "rows", start: number, } | { "type": "columns", start: number, };

export type RichProjectionPatch = { scope: RichProjectionPatchScope, projection: ReadOnlyRichProjection, };

export type SheetStructureMetadataPatch = { merges: Array<MergeRange>, columnWidths?: { [key in number]: number }, rowHeights?: { [key in number]: number }, rich: RichProjectionPatch, };

export type RowInsertedPatch = { sheetIndex: number, rowIndex: number, rows: Array<Array<CellValue>>, metadata: SheetStructureMetadataPatch, };

export type RowDeletedPatch = { sheetIndex: number, rowIndex: number, count: number, metadata: SheetStructureMetadataPatch, };

export type ColumnInsertedPatch = { sheetIndex: number, colIndex: number, values: Array<CellValue>, metadata: SheetStructureMetadataPatch, };

export type ColumnDeletedPatch = { sheetIndex: number, colIndex: number, count: number, metadata: SheetStructureMetadataPatch, };

export type SheetShapePatch = { sheetIndex: number, rowLengths: Array<number>, };

export type ResyncRequiredPatch = { reason: string, };

export type EditorPatch = { "type": "Cells", "data": { changes: Array<SheetCellChange>, } } | { "type": "Layout", "data": { patch: LayoutPatch, } } | { "type": "SheetInserted", "data": { patch: SheetInsertedPatch, } } | { "type": "SheetDeleted", "data": { patch: SheetDeletedPatch, } } | { "type": "SheetUpdated", "data": { patch: SheetUpdatedPatch, } } | { "type": "SheetsReplaced", "data": { patch: SheetsReplacedPatch, } } | { "type": "RowInserted", "data": { patch: RowInsertedPatch, } } | { "type": "RowDeleted", "data": { patch: RowDeletedPatch, } } | { "type": "ColumnInserted", "data": { patch: ColumnInsertedPatch, } } | { "type": "ColumnDeleted", "data": { patch: ColumnDeletedPatch, } } | { "type": "SheetShape", "data": { patch: SheetShapePatch, } } | { "type": "ResyncRequired", "data": { patch: ResyncRequiredPatch, } };

export type EditorMutationResponse = { protocolVersion: 1, documentId: number, revision: number, formulaStatus: FormulaStatus, capabilities: WorkbookCapabilities, editorState: EditorStateInfo, patches?: Array<EditorPatch>, };

export type EditorSessionInfo = { documentId: number, revision: number, formulaStatus: FormulaStatus, capabilities: WorkbookCapabilities, editorState: EditorStateInfo, };

export type OpenDocumentResponse = { fileData: FileData, editorSession: EditorSessionInfo, };

export type SavedDocumentResponse = { fileData: FileData, editorSession: EditorSessionInfo, };

