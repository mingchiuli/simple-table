// Generated from Rust editor contract by ts-rs. Do not edit by hand.

export type U64String = `${bigint}`;

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

export type SheetExtent = { rowCount: number, columnCount: number, };

export type FileData = { path: string, fileName: string, sheets: Array<SheetData>, };

export type SheetLayoutProjection = { columnWidths?: { [key in number]: number }, rowHeights?: { [key in number]: number }, };

export type SheetLayoutUpdate = { sheetIndex: number, layout: SheetLayoutProjection, };

export type SheetManifest = { name: string, extent: SheetExtent, layout: SheetLayoutProjection, };

export type DocumentManifest = { path: string, fileName: string, sheets: Array<SheetManifest>, };

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

export type SpreadsheetFormatOptions = { defaultExtension: string, supportedExtensions: Array<string>, };

export type SheetCellChange = { sheetIndex: number, row: number, col: number, value: CellValue, };

export type SetCellRequest = { sheetIndex: number, row: number, col: number, text: string, };

export type SearchResult = { sheetIndex: number, sheetName: string, row: number, col: number, value: string, cellPosition: string, };

export type SearchResponse = { results: Array<SearchResult>, truncated: boolean, };

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

export type AddRecentFileRequest = { originalPath?: string, documentId: U64String, baseRevision: U64String, };

export type HistoryStatus = { isTruncated: boolean, reason?: string, undoEntries: number, redoEntries: number, undoEstimatedBytes: number, redoEstimatedBytes: number, maxHistoryBytes: number, maxSingleEntryBytes: number, };

export type EditorStateInfo = { canUndo: boolean, canRedo: boolean, isDirty: boolean, history: HistoryStatus, };

export type FormulaIssueKind = "invalidFormula" | "volatileFormula" | "unsupportedDependency" | "largeRangeDependency";

export type FormulaIssue = { sheetIndex: number, row: number, col: number, kind: FormulaIssueKind, message: string, };

export type FormulaDiagnostics = { invalidFormulaCount: number, volatileFormulaCount: number, unsupportedDependencyCount: number, largeRangeDependencyCount: number, skippedReferenceRewriteCount: number, issues?: Array<FormulaIssue>, };

export type FormulaStatus = { "state": "ready", diagnostics: FormulaDiagnostics, } | { "state": "degraded", message: string, diagnostics: FormulaDiagnostics, };

export type LayoutPatch = { sheetIndex: number, columnWidths?: { [key in number]: number | null }, rowHeights?: { [key in number]: number | null }, };

export type SheetInsertedPatch = { sheetIndex: number, sheet: SheetManifest, };

export type SheetDeletedPatch = { sheetIndex: number, };

export type SheetInvalidatedPatch = { sheetIndex: number, };

export type SheetsReplacedPatch = { startIndex: number, sheets: Array<SheetManifest>, };

export type RowInsertedPatch = { sheetIndex: number, rowIndex: number, count: number, };

export type RowDeletedPatch = { sheetIndex: number, rowIndex: number, count: number, };

export type ColumnInsertedPatch = { sheetIndex: number, colIndex: number, count: number, };

export type ColumnDeletedPatch = { sheetIndex: number, colIndex: number, count: number, };

export type ResyncRequiredPatch = { reason: string, };

export type EditorPatch = { "type": "Cells", "data": { changes: Array<SheetCellChange>, } } | { "type": "Layout", "data": { patch: LayoutPatch, } } | { "type": "SheetInserted", "data": { patch: SheetInsertedPatch, } } | { "type": "SheetDeleted", "data": { patch: SheetDeletedPatch, } } | { "type": "SheetInvalidated", "data": { patch: SheetInvalidatedPatch, } } | { "type": "SheetsReplaced", "data": { patch: SheetsReplacedPatch, } } | { "type": "RowInserted", "data": { patch: RowInsertedPatch, } } | { "type": "RowDeleted", "data": { patch: RowDeletedPatch, } } | { "type": "ColumnInserted", "data": { patch: ColumnInsertedPatch, } } | { "type": "ColumnDeleted", "data": { patch: ColumnDeletedPatch, } } | { "type": "ResyncRequired", "data": { patch: ResyncRequiredPatch, } };

export type EditorCommandContext = { documentId: U64String, baseRevision: U64String, };

export type EditorMutationResponse = { protocolVersion: 3, documentId: U64String, revision: U64String, formulaStatus: FormulaStatus, capabilities: WorkbookCapabilities, editorState: EditorStateInfo, patches?: Array<EditorPatch>, sheetExtents?: Array<SheetExtent>, sheetLayouts?: Array<SheetLayoutUpdate>, };

export type EditorSessionInfo = { documentId: U64String, revision: U64String, formulaStatus: FormulaStatus, capabilities: WorkbookCapabilities, editorState: EditorStateInfo, };

export type OpenDocumentResponse = { document: DocumentManifest, editorSession: EditorSessionInfo, initialRegion?: SheetRegionProjectionResponse, };

export type SheetRegion = { sheetIndex: number, rowStart: number, rowEnd: number, colStart: number, colEnd: number, };

export type SheetRegionMetadata = { merges?: Array<MergeRange>, cellFormats?: { [key in string]: CellFormatProjection }, cellStyles?: { [key in string]: CellStyleProjection }, };

export type SheetRegionProjectionResponse = { documentId: U64String, revision: U64String, region: SheetRegion, cells: Array<SheetCellChange>, mergeAnchorCells?: Array<SheetCellChange>, metadata: SheetRegionMetadata, estimatedBytes?: number, };

export type PreparedOpenDocument = { token: string, };

export type SavedDocumentIdentity = { path: string, fileName: string, };

export type SavedDocumentResponse = { document?: DocumentManifest, identity?: SavedDocumentIdentity, editorSession: EditorSessionInfo, };

export type TauriCommandMap = {
  "pick_open_file_desktop": { args: Record<string, never>, result: { path: string, fileName: string } | null },
  "discard_open_file_selection_desktop": { args: { path: string }, result: void },
  "prepare_open_file_desktop": { args: { path: string }, result: PreparedOpenDocument },
  "prepare_recent_file_desktop": { args: { id: string }, result: PreparedOpenDocument },
  "pick_save_location_desktop": { args: { defaultName: string }, result: string | null },
  "discard_save_location_desktop": { args: { path: string }, result: void },
  "save_file_desktop": { args: { path: string, documentId: U64String, baseRevision: U64String }, result: SavedDocumentResponse },
  "export_file_desktop": { args: { defaultName: string, documentId: U64String, baseRevision: U64String }, result: string | null },
  "prepare_new_file": { args: Record<string, never>, result: PreparedOpenDocument },
  "commit_prepared_document": { args: { token: string, expectedDocumentId: U64String | null, expectedRevision: U64String | null }, result: OpenDocumentResponse },
  "abort_prepared_document": { args: { token: string }, result: void },
  "get_active_document": { args: Record<string, never>, result: OpenDocumentResponse | null },
  "get_mutation_result": { args: { documentId: U64String, commandId: string }, result: EditorMutationResponse | null },
  "get_current_document_projection": { args: { documentId: U64String, baseRevision: U64String, preferredSheetIndex: number }, result: OpenDocumentResponse },
  "get_sheet_region_projection": { args: { documentId: U64String, baseRevision: U64String, region: SheetRegion }, result: SheetRegionProjectionResponse },
  "close_current_document": { args: { documentId: U64String }, result: void },
  "get_document_capabilities": { args: { documentId: U64String, baseRevision: U64String }, result: DocumentCapabilities },
  "get_native_save_plan": { args: { documentId: U64String, baseRevision: U64String, targetPathOrName: string }, result: NativeSavePlan },
  "get_spreadsheet_format_options": { args: Record<string, never>, result: SpreadsheetFormatOptions },
  "get_editor_state": { args: { documentId: U64String | null, baseRevision: U64String | null }, result: EditorSessionInfo | null },
  "undo": { args: { documentId: U64String, baseRevision: U64String, commandId: string }, result: EditorMutationResponse },
  "redo": { args: { documentId: U64String, baseRevision: U64String, commandId: string }, result: EditorMutationResponse },
  "set_cell": { args: { documentId: U64String, baseRevision: U64String, commandId: string, sheetIndex: number, row: number, col: number, text: string }, result: EditorMutationResponse },
  "set_cells": { args: { documentId: U64String, baseRevision: U64String, commandId: string, changes: Array<SetCellRequest> }, result: EditorMutationResponse },
  "add_row": { args: { documentId: U64String, baseRevision: U64String, commandId: string, sheetIndex: number, rowIndex: number }, result: EditorMutationResponse },
  "delete_row": { args: { documentId: U64String, baseRevision: U64String, commandId: string, sheetIndex: number, rowIndex: number }, result: EditorMutationResponse },
  "add_column": { args: { documentId: U64String, baseRevision: U64String, commandId: string, sheetIndex: number, colIndex: number }, result: EditorMutationResponse },
  "delete_column": { args: { documentId: U64String, baseRevision: U64String, commandId: string, sheetIndex: number, colIndex: number }, result: EditorMutationResponse },
  "set_column_width": { args: { documentId: U64String, baseRevision: U64String, commandId: string, sheetIndex: number, colIndex: number, width: number | null }, result: EditorMutationResponse },
  "set_row_height": { args: { documentId: U64String, baseRevision: U64String, commandId: string, sheetIndex: number, rowIndex: number, height: number | null }, result: EditorMutationResponse },
  "add_sheet": { args: { documentId: U64String, baseRevision: U64String, commandId: string }, result: EditorMutationResponse },
  "delete_sheet": { args: { documentId: U64String, baseRevision: U64String, commandId: string, sheetIndex: number }, result: EditorMutationResponse },
  "search": { args: { documentId: U64String, baseRevision: U64String, query: string, scope: SearchScope, currentSheetIndex: number | null }, result: SearchResponse },
  "get_recent_files": { args: Record<string, never>, result: Array<RecentFile> },
  "remove_recent_file": { args: { id: string }, result: void },
  "add_recent_file_with_thumbnail": { args: { request: AddRecentFileRequest }, result: RecentFile },
  "pick_open_file_android": { args: Record<string, never>, result: { path: string, originalPath: string, fileName: string } | null },
  "discard_open_file_selection_android": { args: { path: string }, result: void },
  "discard_save_location_android": { args: { path: string }, result: void },
  "prepare_open_file_android": { args: { path: string }, result: PreparedOpenDocument },
  "save_file_android": { args: { path: string, documentId: U64String, baseRevision: U64String }, result: SavedDocumentResponse },
  "export_file_android": { args: { defaultName: string, documentId: U64String, baseRevision: U64String }, result: string | null },
  "pick_save_location_android": { args: { defaultName: string }, result: string | null },
  "pick_open_file_ios": { args: Record<string, never>, result: { path: string, originalPath: string, fileName: string } | null },
  "discard_open_file_selection_ios": { args: { path: string }, result: void },
  "discard_save_location_ios": { args: { path: string }, result: void },
  "prepare_open_file_ios": { args: { path: string }, result: PreparedOpenDocument },
  "pick_save_location_ios": { args: { defaultName: string }, result: string | null },
  "save_file_ios": { args: { path: string, documentId: U64String, baseRevision: U64String }, result: SavedDocumentResponse },
  "export_file_ios": { args: { defaultName: string, documentId: U64String, baseRevision: U64String }, result: string | null },
  "check_update_mobile": { args: { currentVersion: string }, result: { version: string, tag_name: string, release_url: string, apk_url: string | null } | null },
}

