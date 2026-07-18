import { beforeEach, describe, expect, it } from 'vitest';
import { createPinia, setActivePinia } from 'pinia';
import { createDocumentSessionCoordinator } from '@/application/documentSessionCoordinator';
import { useDocumentSessionStore } from '@/stores/documentSession';
import { useDocumentStatusStore } from '@/stores/documentStatus';
import { useEditorSelectionStore } from '@/stores/editorSelection';
import { usePendingCellSavesStore } from '@/stores/pendingCellSaves';
import { useSearchSessionStore } from '@/stores/searchSession';
import { openResponseFromFileData, savedResponseFromFileData } from '@/test/documentFixtures';
import {
  defaultHistoryStatus,
  defaultRichProjection,
  defaultWorkbookCapabilities,
  readyFormulaStatus,
  type CellValue,
  type EditorMutationResponse,
  type EditorSessionInfo,
  type FileData,
} from '@/types';

describe('documentSessionCoordinator', () => {
  beforeEach(() => setActivePinia(createPinia()));

  it('commits an opened document across every session-owned store', () => {
    const coordinator = coordinatorForStores();
    const status = useDocumentStatusStore();
    const selection = useEditorSelectionStore();
    const pending = usePendingCellSavesStore();
    const search = useSearchSessionStore();
    status.markPendingContentChange();
    selection.selectCell(9, 9);
    pending.setDraft('0:0:0', 'draft');
    search.beginSearch('old');

    coordinator.openDocumentResponse(opened('0'), '/tmp/book.xlsx');

    expect(useDocumentSessionStore().currentFilePath).toBe('/tmp/book.xlsx');
    expect(status.canUndo).toBe(true);
    expect(status.hasPendingContentChange).toBe(false);
    expect(selection.selectedCell).toBeNull();
    expect(Object.keys(pending.draftCellValues)).toHaveLength(0);
    expect(search.searchQuery).toBe('');
    expect(search.isSearching).toBe(false);
  });

  it('does not clear local state when a saved response misses its command context', () => {
    const coordinator = coordinatorForStores();
    const document = useDocumentSessionStore();
    const status = useDocumentStatusStore();
    const pending = usePendingCellSavesStore();
    coordinator.openDocumentResponse(opened('0'));
    const context = document.requireCommandContext();
    pending.setDraft('0:0:0', 'draft');
    status.markPendingContentChange();
    document.revision = '1';

    const applied = coordinator.applySavedDocumentResponseForContext(
      context,
      savedResponseFromFileData(fileData('saved'), session('1'))
    );

    expect(applied).toBe(false);
    expect(pending.draftCellValues['0:0:0']).toBe('draft');
    expect(status.hasPendingContentChange).toBe(true);
  });

  it('restores UI state and locks the committed revision when resync fails', async () => {
    const coordinator = coordinatorForStores();
    const document = useDocumentSessionStore();
    const status = useDocumentStatusStore();
    const selection = useEditorSelectionStore();
    const search = useSearchSessionStore();
    coordinator.openDocumentResponse(opened('0'));
    selection.selectCell(0, 0);
    search.beginSearch('needle');
    search.applySearchResults({
      results: [{
        sheetIndex: 0,
        sheetName: 'Sheet1',
        row: 0,
        col: 0,
        value: 'old',
        cellPosition: 'A1',
      }],
      truncated: false,
    });

    await expect(coordinator.applyMutationResponseWithResync(
      mutation('1'),
      async () => { throw new Error('projection unavailable'); }
    )).rejects.toThrow('projection unavailable');

    expect(document.revision).toBe('1');
    expect(document.projectionStale).toBe(true);
    expect(selection.selectedCell).toEqual({ row: 0, col: 0 });
    expect(search.searchResults).toEqual([]);
    expect(status.canRedo).toBe(true);
    expect(status.isContentDirty).toBe(true);
  });
});

function coordinatorForStores() {
  return createDocumentSessionCoordinator({
    document: useDocumentSessionStore(),
    status: useDocumentStatusStore(),
    selection: useEditorSelectionStore(),
    pending: usePendingCellSavesStore(),
    search: useSearchSessionStore(),
  });
}

function opened(revision: `${bigint}` = '0') {
  return openResponseFromFileData(fileData('old'), session(revision));
}

function fileData(value: string): FileData {
  const cell: CellValue = { type: 'cell', kind: 'text', raw: value, display: value };
  return {
    path: '/tmp/book.xlsx',
    fileName: 'book.xlsx',
    sheets: [{
      name: 'Sheet1',
      rows: [[cell]],
      merges: [],
      rich: defaultRichProjection(),
    }],
  };
}

function session(revision: `${bigint}`): EditorSessionInfo {
  return {
    documentId: '1',
    revision,
    formulaStatus: readyFormulaStatus(),
    capabilities: defaultWorkbookCapabilities(),
    editorState: {
      canUndo: true,
      canRedo: false,
      isDirty: false,
      history: defaultHistoryStatus(),
    },
  };
}

function mutation(revision: `${bigint}`): EditorMutationResponse {
  return {
    protocolVersion: 4,
    documentId: '1',
    revision,
    formulaStatus: readyFormulaStatus(),
    capabilities: defaultWorkbookCapabilities(),
    editorState: {
      canUndo: true,
      canRedo: true,
      isDirty: true,
      history: defaultHistoryStatus(),
    },
    patches: [{
      type: 'ResyncRequired',
      data: { patch: { reason: 'test' } },
    }],
  };
}
