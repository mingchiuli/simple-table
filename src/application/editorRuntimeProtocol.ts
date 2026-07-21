import type {
  EditorPatch,
  EditorSessionInfo,
  EditorStateInfo,
  FormulaStatus,
  SearchResponse,
  WorkbookCapabilities,
} from '@/types/protocol';
import type {
  DocumentStatusStateInput,
  RuntimeFormulaStatus,
  RuntimeHistoryStatus,
  RuntimeWorkbookCapabilities,
  SearchOutcomeStateInput,
  SelectionTransform,
} from '@/types/editorRuntime';

export function editorSessionStatusState(info: EditorSessionInfo): DocumentStatusStateInput {
  return editorStatusState(info.editorState, info.formulaStatus, info.capabilities);
}

export function editorStatusState(
  state: EditorStateInfo,
  formulaStatus: FormulaStatus,
  capabilities: WorkbookCapabilities,
): DocumentStatusStateInput {
  return {
    canUndo: state.canUndo,
    canRedo: state.canRedo,
    isContentDirty: state.isDirty,
    formulaStatus: runtimeFormulaStatus(formulaStatus),
    capabilities: runtimeWorkbookCapabilities(capabilities),
    history: runtimeHistoryStatus(state.history),
  };
}

export function searchOutcomeState(response: SearchResponse): SearchOutcomeStateInput {
  return {
    results: response.results.map((result) => ({ ...result })),
    truncated: response.truncated,
  };
}

export function selectionTransforms(patches: EditorPatch[] | undefined): SelectionTransform[] {
  return (patches ?? []).flatMap((patch): SelectionTransform[] => {
    switch (patch.type) {
      case 'SheetInserted':
        return [{ type: 'sheetInserted', sheetIndex: patch.data.patch.sheetIndex }];
      case 'SheetDeleted':
        return [{ type: 'sheetDeleted', sheetIndex: patch.data.patch.sheetIndex }];
      case 'SheetsReplaced':
        return [{ type: 'sheetsReplaced', startIndex: patch.data.patch.startIndex }];
      case 'RowInserted':
        return [{ type: 'rowInserted', ...patch.data.patch }];
      case 'RowDeleted':
        return [{ type: 'rowDeleted', ...patch.data.patch }];
      case 'ColumnInserted':
        return [{ type: 'columnInserted', ...patch.data.patch }];
      case 'ColumnDeleted':
        return [{ type: 'columnDeleted', ...patch.data.patch }];
      default:
        return [];
    }
  });
}

function runtimeFormulaStatus(status: FormulaStatus): RuntimeFormulaStatus {
  const diagnostics = {
    invalidFormulaCount: status.diagnostics.invalidFormulaCount,
    volatileFormulaCount: status.diagnostics.volatileFormulaCount,
    unsupportedDependencyCount: status.diagnostics.unsupportedDependencyCount,
    largeRangeDependencyCount: status.diagnostics.largeRangeDependencyCount,
    skippedReferenceRewriteCount: status.diagnostics.skippedReferenceRewriteCount,
    issues: (status.diagnostics.issues ?? []).map((issue) => ({ ...issue })),
  };
  return status.state === 'degraded'
    ? { state: 'degraded', message: status.message, diagnostics }
    : { state: 'ready', diagnostics };
}

export function runtimeWorkbookCapabilities(
  capabilities: WorkbookCapabilities,
): RuntimeWorkbookCapabilities {
  return {
    save: {
      canNativeSave: capabilities.save.canNativeSave,
      blockedSaveReasons: [...(capabilities.save.blockedSaveReasons ?? [])],
      detectedFeatures: [...(capabilities.save.detectedFeatures ?? [])],
    },
    structure: {
      canInsertDeleteSheets: capabilities.structure.canInsertDeleteSheets,
      blockedStructureReasons: [...(capabilities.structure.blockedStructureReasons ?? [])],
      blockedSheetStructureReasons: [
        ...(capabilities.structure.blockedSheetStructureReasons ?? []),
      ],
    },
    rich: { ...capabilities.rich },
    sheets: (capabilities.sheets ?? []).map((sheet) => ({
      canEditCells: sheet.canEditCells,
      canResizeRowsColumns: sheet.canResizeRowsColumns,
      canInsertDeleteRows: sheet.canInsertDeleteRows,
      canInsertDeleteColumns: sheet.canInsertDeleteColumns,
      blockedEditReasons: [...(sheet.blockedEditReasons ?? [])],
      blockedResizeReasons: [...(sheet.blockedResizeReasons ?? [])],
      blockedRowStructureReasons: [...(sheet.blockedRowStructureReasons ?? [])],
      blockedColumnStructureReasons: [...(sheet.blockedColumnStructureReasons ?? [])],
    })),
  };
}

function runtimeHistoryStatus(history: EditorStateInfo['history']): RuntimeHistoryStatus {
  return {
    isTruncated: history.isTruncated,
    reason: history.reason,
    undoEntries: history.undoEntries,
    redoEntries: history.redoEntries,
    undoEstimatedBytes: history.undoEstimatedBytes,
    redoEstimatedBytes: history.redoEstimatedBytes,
    maxHistoryBytes: history.maxHistoryBytes,
    maxSingleEntryBytes: history.maxSingleEntryBytes,
  };
}
