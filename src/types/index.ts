export * from './generated';
export type * from './updateRuntime';
export type * from './documentRuntime';
export type * from './pendingCellSave';

import type {
  FormulaDiagnostics,
  FormulaStatus,
  HistoryStatus,
  EditorCommandContext,
  CellValue,
  ReadOnlyRichProjection,
  SheetRegionMetadata,
  SheetCapabilities,
  SheetExtent,
  SheetRegion,
  WorkbookCapabilities,
} from './generated';

export type SheetLayoutState = {
  columnWidths: Record<number, number>;
  rowHeights: Record<number, number>;
};

export type MutationCommandContext = EditorCommandContext & {
  commandId: string;
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
  merges: NonNullable<SheetRegionMetadata['merges']>;
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

export function defaultSheetCapabilities(): SheetCapabilities {
  return {
    canEditCells: true,
    canResizeRowsColumns: true,
    canInsertDeleteRows: true,
    canInsertDeleteColumns: true,
    blockedEditReasons: [],
    blockedResizeReasons: [],
    blockedRowStructureReasons: [],
    blockedColumnStructureReasons: [],
  };
}

export function defaultWorkbookCapabilities(): WorkbookCapabilities {
  return {
    save: {
      canNativeSave: true,
      blockedSaveReasons: [],
      detectedFeatures: [],
    },
    structure: {
      canInsertDeleteSheets: true,
      blockedStructureReasons: [],
      blockedSheetStructureReasons: [],
    },
    rich: {
      canEditStyles: false,
      canEditDrawings: false,
      canEditHyperlinks: false,
    },
    sheets: [],
  };
}

export function workbookSheetCapabilities(
  workbook: WorkbookCapabilities,
  sheetIndex: number
): SheetCapabilities {
  const sheetCapabilities = workbook.sheets?.[sheetIndex];
  if (sheetCapabilities) {
    return sheetCapabilities;
  }
  return defaultSheetCapabilities();
}

export function defaultHistoryStatus(): HistoryStatus {
  return {
    isTruncated: false,
    undoEntries: 0,
    redoEntries: 0,
    undoEstimatedBytes: 0,
    redoEstimatedBytes: 0,
    maxHistoryBytes: 0,
    maxSingleEntryBytes: 0,
  };
}

export function defaultFormulaDiagnostics(): FormulaDiagnostics {
  return {
    invalidFormulaCount: 0,
    volatileFormulaCount: 0,
    unsupportedDependencyCount: 0,
    largeRangeDependencyCount: 0,
    skippedReferenceRewriteCount: 0,
    issues: [],
  };
}

export function readyFormulaStatus(): FormulaStatus {
  return {
    state: 'ready',
    diagnostics: defaultFormulaDiagnostics(),
  };
}

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
