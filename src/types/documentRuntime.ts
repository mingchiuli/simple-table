export type U64String = `${bigint}`;

export type ScalarCellValue = string | number | boolean | null;

export type CellKind = 'blank' | 'text' | 'number' | 'boolean' | 'formula' | 'error';

export type CellFormatProjection = {
  numberFormat?: string | null;
  styleId?: string | null;
};

export type CellFormulaProjection = {
  formula: string;
  cachedValue: CellValue;
  error?: string;
};

export type CellValue = {
  type: 'cell';
  kind: CellKind;
  raw: ScalarCellValue;
  display: string;
  formula?: CellFormulaProjection;
  format?: CellFormatProjection;
};

export type MergeRange = {
  startRow: number;
  startCol: number;
  endRow: number;
  endCol: number;
};

export type SheetExtent = {
  rowCount: number;
  columnCount: number;
};

export type SheetLayoutProjection = {
  columnWidths?: Record<number, number>;
  rowHeights?: Record<number, number>;
};

export type SheetManifest = {
  name: string;
  extent: SheetExtent;
  layout: SheetLayoutProjection;
};

export type DocumentManifest = {
  path: string;
  fileName: string;
  sheets: SheetManifest[];
};

export type CellStyleProjection = {
  fontColor?: string | null;
  backgroundColor?: string | null;
  bold?: boolean | null;
  italic?: boolean | null;
  horizontalAlign?: string | null;
  verticalAlign?: string | null;
  numberFormat?: string | null;
};

export type FreezePaneProjection = {
  topLeftCell: string;
  horizontalSplit: number;
  verticalSplit: number;
  activePane: string;
  state: string;
};

export type HyperlinkProjection = {
  url: string;
  tooltip?: string | null;
  location: boolean;
};

export type DrawingKind = 'image' | 'chart';

export type DrawingProjection = {
  kind: DrawingKind;
  fromRow: number;
  fromCol: number;
  toRow?: number | null;
  toCol?: number | null;
};

export type ReadOnlyRichProjection = {
  cellFormats?: Record<string, CellFormatProjection>;
  cellStyles?: Record<string, CellStyleProjection>;
  hiddenRows?: number[];
  hiddenColumns?: number[];
  freezePane?: FreezePaneProjection | null;
  hyperlinks?: Record<string, HyperlinkProjection>;
  drawings?: DrawingProjection[];
  hasMoreDrawings: boolean;
  hasStyleMetadata: boolean;
  hasHyperlinks: boolean;
  hasFreezePane: boolean;
};

export type EditorCommandContext = {
  documentId: U64String;
  baseRevision: U64String;
};

export type MutationCommandContext = EditorCommandContext & {
  commandId: string;
};

export type SheetRegion = {
  sheetIndex: number;
  rowStart: number;
  rowEnd: number;
  colStart: number;
  colEnd: number;
};

export type SheetRegionMetadata = {
  merges: MergeRange[];
  cellFormats: Record<string, CellFormatProjection>;
  cellStyles: Record<string, CellStyleProjection>;
};

export type SheetCellChange = {
  sheetIndex: number;
  row: number;
  col: number;
  value: CellValue;
};

export type SheetRegionProjection = {
  region: SheetRegion;
  cells: SheetCellChange[];
  mergeAnchorCells: SheetCellChange[];
  metadata: SheetRegionMetadata;
  estimatedBytes?: number;
};

export type LayoutPatch = {
  sheetIndex: number;
  columnWidths?: Record<number, number | null>;
  rowHeights?: Record<number, number | null>;
};

export type EditorPatch =
  | { type: 'Cells'; data: { changes: SheetCellChange[] } }
  | { type: 'Layout'; data: { patch: LayoutPatch } }
  | { type: 'SheetInserted'; data: { patch: { sheetIndex: number; sheet: SheetManifest } } }
  | { type: 'SheetDeleted'; data: { patch: { sheetIndex: number } } }
  | { type: 'SheetInvalidated'; data: { patch: { sheetIndex: number } } }
  | { type: 'SheetsReplaced'; data: { patch: { startIndex: number; sheets: SheetManifest[] } } }
  | { type: 'RowInserted'; data: { patch: RowStructurePatch } }
  | { type: 'RowDeleted'; data: { patch: RowStructurePatch } }
  | { type: 'ColumnInserted'; data: { patch: ColumnStructurePatch } }
  | { type: 'ColumnDeleted'; data: { patch: ColumnStructurePatch } }
  | { type: 'ResyncRequired'; data: { patch: { reason: string } } };

type RowStructurePatch = {
  sheetIndex: number;
  rowIndex: number;
  count: number;
};

type ColumnStructurePatch = {
  sheetIndex: number;
  colIndex: number;
  count: number;
};

export type DocumentSessionLifecycle = 'idle' | 'loading' | 'saving' | 'closing';

export type SheetLayoutState = {
  columnWidths: Record<number, number>;
  rowHeights: Record<number, number>;
};

export type LoadedSheetSlot = {
  state: 'loaded';
  name: string;
  extent: SheetExtent;
  layout: SheetLayoutState;
  blocks: SheetRegionBlock[];
  metadata: LoadedSheetRegionMetadata;
};

export type LoadedSheetRegionMetadata = {
  merges: MergeRange[];
  rich: ReadOnlyRichProjection;
};

export type SheetRegionBlock = {
  key: string;
  region: SheetRegion;
  cells: Record<string, CellValue>;
  mergeAnchorCells: Record<string, CellValue>;
  metadata: SheetRegionMetadata;
  estimatedBytes: number;
};

export type UnloadedSheetSlot = {
  state: 'unloaded';
  name: string;
  extent: SheetExtent;
  layout: SheetLayoutState;
};

export type SheetSlot = LoadedSheetSlot | UnloadedSheetSlot;

export type DocumentProjection = {
  path: string;
  fileName: string;
  sheets: SheetSlot[];
};

export type DocumentSessionStateInput = {
  data: DocumentProjection;
  currentFilePath: string | null;
  documentId: U64String;
  revision: U64String;
  preferredSheetIndex: number;
  activatePreferredSheet: boolean;
  resetEditorCommandDepth: boolean;
  preserveResidentSheetOrder: boolean;
};

export type DocumentMutationStateInput = {
  data: DocumentProjection | null;
  documentId: U64String;
  revision: U64String;
  resyncRequired: boolean;
};

export type DocumentIdentityStateInput = {
  documentId: U64String;
  revision: U64String;
};

export function defaultRichProjection(): ReadOnlyRichProjection {
  return {
    cellFormats: {},
    cellStyles: {},
    hiddenRows: [],
    hiddenColumns: [],
    freezePane: undefined,
    hyperlinks: {},
    drawings: [],
    hasMoreDrawings: false,
    hasStyleMetadata: false,
    hasHyperlinks: false,
    hasFreezePane: false,
  };
}
