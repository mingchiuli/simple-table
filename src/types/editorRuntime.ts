export type RuntimeFormulaIssueKind =
  | 'invalidFormula'
  | 'volatileFormula'
  | 'unsupportedDependency'
  | 'largeRangeDependency';

export type RuntimeFormulaIssue = {
  sheetIndex: number;
  row: number;
  col: number;
  kind: RuntimeFormulaIssueKind;
  message: string;
};

export type RuntimeFormulaDiagnostics = {
  invalidFormulaCount: number;
  volatileFormulaCount: number;
  unsupportedDependencyCount: number;
  largeRangeDependencyCount: number;
  skippedReferenceRewriteCount: number;
  issues: RuntimeFormulaIssue[];
};

export type RuntimeFormulaStatus =
  | { state: 'ready'; diagnostics: RuntimeFormulaDiagnostics }
  | { state: 'degraded'; message: string; diagnostics: RuntimeFormulaDiagnostics };

export type RuntimeSheetCapabilities = {
  canEditCells: boolean;
  canResizeRowsColumns: boolean;
  canInsertDeleteRows: boolean;
  canInsertDeleteColumns: boolean;
  blockedEditReasons: string[];
  blockedResizeReasons: string[];
  blockedRowStructureReasons: string[];
  blockedColumnStructureReasons: string[];
};

export type RuntimeWorkbookCapabilities = {
  save: {
    canNativeSave: boolean;
    blockedSaveReasons: string[];
    detectedFeatures: string[];
  };
  structure: {
    canInsertDeleteSheets: boolean;
    blockedStructureReasons: string[];
    blockedSheetStructureReasons: string[];
  };
  rich: {
    canEditStyles: boolean;
    canEditDrawings: boolean;
    canEditHyperlinks: boolean;
  };
  sheets: RuntimeSheetCapabilities[];
};

export type RuntimeHistoryStatus = {
  isTruncated: boolean;
  reason?: string;
  undoEntries: number;
  redoEntries: number;
  undoEstimatedBytes: number;
  redoEstimatedBytes: number;
  maxHistoryBytes: number;
  maxSingleEntryBytes: number;
};

export type DocumentStatusStateInput = {
  canUndo: boolean;
  canRedo: boolean;
  isContentDirty: boolean;
  formulaStatus: RuntimeFormulaStatus;
  capabilities: RuntimeWorkbookCapabilities;
  history: RuntimeHistoryStatus;
};

export type EditorStateStateInput = Pick<
  DocumentStatusStateInput,
  'canUndo' | 'canRedo' | 'isContentDirty' | 'history'
>;

export type RuntimeSearchResult = {
  sheetIndex: number;
  sheetName: string;
  row: number;
  col: number;
  value: string;
  cellPosition: string;
};

export type SearchResult = RuntimeSearchResult;
export type SearchScope = 'currentSheet' | 'allSheets';
export type FormulaStatus = RuntimeFormulaStatus;
export type HistoryStatus = RuntimeHistoryStatus;

export type SearchOutcomeStateInput = {
  results: RuntimeSearchResult[];
  truncated: boolean;
};

export type SearchSessionSnapshot = {
  searchResults: RuntimeSearchResult[];
  searchResultsTruncated: boolean;
  searchQuery: string;
  isSearching: boolean;
};

export type SelectionTransform =
  | { type: 'sheetInserted'; sheetIndex: number }
  | { type: 'sheetDeleted'; sheetIndex: number }
  | { type: 'sheetsReplaced'; startIndex: number }
  | { type: 'rowInserted'; sheetIndex: number; rowIndex: number; count: number }
  | { type: 'rowDeleted'; sheetIndex: number; rowIndex: number; count: number }
  | { type: 'columnInserted'; sheetIndex: number; colIndex: number; count: number }
  | { type: 'columnDeleted'; sheetIndex: number; colIndex: number; count: number };

export function defaultSheetCapabilities(): RuntimeSheetCapabilities {
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

export function defaultWorkbookCapabilities(): RuntimeWorkbookCapabilities {
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
  workbook: RuntimeWorkbookCapabilities,
  sheetIndex: number,
): RuntimeSheetCapabilities {
  return workbook.sheets[sheetIndex] ?? defaultSheetCapabilities();
}

export function defaultHistoryStatus(): RuntimeHistoryStatus {
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

export function defaultFormulaDiagnostics(): RuntimeFormulaDiagnostics {
  return {
    invalidFormulaCount: 0,
    volatileFormulaCount: 0,
    unsupportedDependencyCount: 0,
    largeRangeDependencyCount: 0,
    skippedReferenceRewriteCount: 0,
    issues: [],
  };
}

export function readyFormulaStatus(): RuntimeFormulaStatus {
  return {
    state: 'ready',
    diagnostics: defaultFormulaDiagnostics(),
  };
}
