import { describe, expect, it } from 'vitest';
import {
  editorSessionStatusState,
  searchOutcomeState,
  selectionTransforms,
} from '@/application/editorRuntimeProtocol';
import type { EditorPatch, EditorSessionInfo, SearchResponse } from '@/types/generated';

describe('editorRuntimeProtocol', () => {
  it('normalizes optional protocol collections before Store admission', () => {
    const session: EditorSessionInfo = {
      documentId: '7',
      revision: '11',
      editorState: {
        canUndo: true,
        canRedo: false,
        isDirty: true,
        history: {
          isTruncated: false,
          undoEntries: 1,
          redoEntries: 0,
          undoEstimatedBytes: 64,
          redoEstimatedBytes: 0,
          maxHistoryBytes: 1024,
          maxSingleEntryBytes: 512,
        },
      },
      formulaStatus: {
        state: 'ready',
        diagnostics: {
          invalidFormulaCount: 0,
          volatileFormulaCount: 0,
          unsupportedDependencyCount: 0,
          largeRangeDependencyCount: 0,
          skippedReferenceRewriteCount: 0,
        },
      },
      capabilities: {
        save: { canNativeSave: true },
        structure: { canInsertDeleteSheets: true },
        rich: {
          canEditStyles: false,
          canEditDrawings: false,
          canEditHyperlinks: false,
        },
      },
    };

    const state = editorSessionStatusState(session);

    expect(state.isContentDirty).toBe(true);
    expect(state.formulaStatus.diagnostics.issues).toEqual([]);
    expect(state.capabilities.save.blockedSaveReasons).toEqual([]);
    expect(state.capabilities.structure.blockedSheetStructureReasons).toEqual([]);
    expect(state.capabilities.sheets).toEqual([]);
  });

  it('copies search results into the runtime contract', () => {
    const response: SearchResponse = {
      results: [
        {
          sheetIndex: 0,
          sheetName: 'Sheet1',
          row: 2,
          col: 3,
          value: 'match',
          cellPosition: 'D3',
        },
      ],
      truncated: true,
    };

    const outcome = searchOutcomeState(response);
    response.results[0]!.value = 'changed';

    expect(outcome.results[0]?.value).toBe('match');
    expect(outcome.truncated).toBe(true);
  });

  it('keeps only structural patches used by selection state', () => {
    const patches: EditorPatch[] = [
      {
        type: 'RowInserted',
        data: { patch: { sheetIndex: 2, rowIndex: 4, count: 3 } },
      },
      { type: 'SheetDeleted', data: { patch: { sheetIndex: 1 } } },
      { type: 'ResyncRequired', data: { patch: { reason: 'stale' } } },
    ];

    expect(selectionTransforms(patches)).toEqual([
      { type: 'rowInserted', sheetIndex: 2, rowIndex: 4, count: 3 },
      { type: 'sheetDeleted', sheetIndex: 1 },
    ]);
    expect(selectionTransforms(undefined)).toEqual([]);
  });
});
