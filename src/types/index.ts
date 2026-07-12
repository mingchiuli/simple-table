export * from './generated';

import type {
  FormulaDiagnostics,
  FormulaStatus,
  HistoryStatus,
  EditorCommandContext,
  CellValue,
  ReadOnlyRichProjection,
  SheetRegionMetadata,
  SheetLayoutProjection,
  SheetCapabilities,
  SheetExtent,
  SheetRegion,
  WorkbookCapabilities,
} from './generated';

export type MutationCommandContext = EditorCommandContext & {
  commandId: string;
};

export type LoadedSheetSlot = {
  state: 'loaded';
  name: string;
  extent: SheetExtent;
  layout: SheetLayoutProjection;
  blocks: SheetRegionBlock[];
};

export type SheetRegionBlock = {
  key: string;
  region: SheetRegion;
  cells: Map<string, CellValue>;
  mergeAnchorCells: Map<string, CellValue>;
  metadata: SheetRegionMetadata;
  estimatedBytes: number;
};

export type UnloadedSheetSlot = {
  state: 'unloaded';
  name: string;
  extent: SheetExtent;
  layout: SheetLayoutProjection;
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
