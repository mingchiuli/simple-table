export * from './generated';

import type {
  FormulaDiagnostics,
  FormulaStatus,
  HistoryStatus,
  ReadOnlyRichProjection,
  SheetCapabilities,
  WorkbookCapabilities,
} from './generated';

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
    canEditCells: true,
    canResizeRowsColumns: true,
    canInsertDeleteRows: true,
    canInsertDeleteColumns: true,
    canInsertDeleteSheets: true,
    canNativeSave: true,
    canEditStyles: false,
    canEditDrawings: false,
    canEditHyperlinks: false,
    blockedStructureReasons: [],
    blockedEditReasons: [],
    blockedResizeReasons: [],
    blockedRowStructureReasons: [],
    blockedColumnStructureReasons: [],
    blockedSheetStructureReasons: [],
    detectedFeatures: [],
    sheetCapabilities: [],
  };
}

export function workbookSheetCapabilities(
  workbook: WorkbookCapabilities,
  sheetIndex: number
): SheetCapabilities {
  const sheetCapabilities = workbook.sheetCapabilities?.[sheetIndex];
  if (sheetCapabilities) {
    return sheetCapabilities;
  }
  return {
    canEditCells: workbook.canEditCells,
    canResizeRowsColumns: workbook.canResizeRowsColumns,
    canInsertDeleteRows: workbook.canInsertDeleteRows,
    canInsertDeleteColumns: workbook.canInsertDeleteColumns,
    blockedEditReasons: workbook.blockedEditReasons ?? [],
    blockedResizeReasons: workbook.blockedResizeReasons ?? [],
    blockedRowStructureReasons: workbook.blockedRowStructureReasons ?? workbook.blockedStructureReasons ?? [],
    blockedColumnStructureReasons:
      workbook.blockedColumnStructureReasons ?? workbook.blockedStructureReasons ?? [],
  };
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
