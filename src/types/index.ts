export * from './generated';

import type {
  FormulaDiagnostics,
  FormulaStatus,
  ReadOnlyRichProjection,
  WorkbookCapabilities,
} from './generated';

export function defaultWorkbookCapabilities(): WorkbookCapabilities {
  return {
    canEditCells: true,
    canResizeRowsColumns: true,
    canInsertDeleteRows: true,
    canInsertDeleteColumns: true,
    canInsertDeleteSheets: true,
    canNativeSave: true,
    blockedStructureReasons: [],
    blockedEditReasons: [],
    blockedResizeReasons: [],
    blockedRowStructureReasons: [],
    blockedColumnStructureReasons: [],
    blockedSheetStructureReasons: [],
    detectedFeatures: [],
  };
}

export function defaultFormulaDiagnostics(): FormulaDiagnostics {
  return {
    invalidFormulaCount: 0,
    volatileFormulaCount: 0,
    unsupportedDependencyCount: 0,
    largeRangeDependencyCount: 0,
    skippedReferenceRewriteCount: 0,
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
    drawings: [],
    hasMoreDrawings: false,
  };
}
