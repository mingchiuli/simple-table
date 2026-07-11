import { beforeEach, describe, expect, it } from 'vitest';
import { createPinia, setActivePinia } from 'pinia';
import { useDocumentSessionStore } from '@/stores/documentSession';
import { defaultWorkbookCapabilities, readyFormulaStatus } from '@/types';
import type {
  EditorMutationResponse,
  OpenDocumentResponse,
  SheetRegionProjectionResponse,
} from '@/types';
import { sheetCell } from '@/stores/documentProjection';
import { blankCell } from '@/utils/cellValue';

describe('documentSession sparse projection', () => {
  beforeEach(() => setActivePinia(createPinia()));

  it('opens from a manifest and stores only the initial region block', () => {
    const store = useDocumentSessionStore();
    store.openDocumentResponse(openResponse());

    expect(store.data?.sheets[0].state).toBe('loaded');
    expect(store.loadedSheet(0)?.blocks).toHaveLength(1);
    expect(sheetCell(store.data?.sheets[0], 0, 0)?.display).toBe('A1');
    expect(store.data?.sheets[1].state).toBe('unloaded');
  });

  it('loads a high row without allocating intermediate row arrays', async () => {
    const store = useDocumentSessionStore();
    store.openDocumentResponse(openResponse());
    const response = regionResponse(0, 199_936, 200_000, 0, 32, 'far');

    expect(await store.ensureSheetRegionLoaded(response.region, async () => response)).toBe(true);
    expect(store.loadedSheet(0)?.blocks).toHaveLength(2);
    expect(sheetCell(store.data?.sheets[0], 199_936, 0)?.display).toBe('far');
  });

  it('deduplicates concurrent tile requests', async () => {
    const store = useDocumentSessionStore();
    store.openDocumentResponse(openResponse());
    const response = regionResponse(0, 128, 256, 0, 32, 'tile');
    let requests = 0;
    const fetch = async () => {
      requests += 1;
      await Promise.resolve();
      return response;
    };

    await Promise.all([
      store.ensureSheetRegionLoaded(response.region, fetch),
      store.ensureSheetRegionLoaded(response.region, fetch),
    ]);
    expect(requests).toBe(1);
  });

  it('evicts old region blocks at the per-sheet budget', async () => {
    const store = useDocumentSessionStore();
    store.openDocumentResponse(openResponse());
    for (let tile = 1; tile <= 10; tile += 1) {
      const response = regionResponse(0, tile * 128, (tile + 1) * 128, 0, 32, `${tile}`);
      await store.ensureSheetRegionLoaded(response.region, async () => response);
    }

    expect(store.loadedSheet(0)?.blocks).toHaveLength(8);
    expect(sheetCell(store.data?.sheets[0], 128, 0)).toBeUndefined();
    expect(sheetCell(store.data?.sheets[0], 1_280, 0)?.display).toBe('10');
  });

  it('invalidates cached blocks for structural patches', () => {
    const store = useDocumentSessionStore();
    store.openDocumentResponse(openResponse());
    store.applyMutationResponse(mutation({
      type: 'RowInserted',
      data: { patch: { sheetIndex: 0, rowIndex: 2, count: 1 } },
    }));

    expect(store.loadedSheet(0)?.blocks).toHaveLength(0);
    expect(store.data?.sheets[0].extent.rowCount).toBe(200_001);
  });

  it('reloads the initial tile when a resident sheet was invalidated', async () => {
    const store = useDocumentSessionStore();
    store.openDocumentResponse(openResponse());
    store.applyMutationResponse(mutation({
      type: 'SheetInvalidated',
      data: { patch: { sheetIndex: 0 } },
    }));
    const response = {
      ...regionResponse(0, 0, 128, 0, 32, 'reloaded'),
      revision: '1' as const,
    };
    let requests = 0;

    const loaded = await store.ensureSheetLoaded(0, async () => {
      requests += 1;
      return response;
    });

    expect(loaded).toBe(true);
    expect(requests).toBe(1);
    expect(sheetCell(store.data?.sheets[0], 0, 0)?.display).toBe('reloaded');
  });

  it('reindexes resident blocks after inserting a sheet', () => {
    const store = useDocumentSessionStore();
    store.openDocumentResponse(openResponse());
    store.applyMutationResponse(mutation({
      type: 'SheetInserted',
      data: {
        patch: {
          sheetIndex: 0,
          sheet: { name: 'Inserted', extent: { rowCount: 0, columnCount: 0 } },
        },
      },
    }, [
      { rowCount: 0, columnCount: 0 },
      { rowCount: 200_000, columnCount: 64 },
      { rowCount: 10, columnCount: 10 },
    ]));

    const shiftedSheet = store.loadedSheet(1);
    expect(shiftedSheet?.blocks).toHaveLength(1);
    expect(shiftedSheet?.blocks[0].region.sheetIndex).toBe(1);
    expect(sheetCell(store.data?.sheets[1], 0, 0)?.display).toBe('A1');
  });

  it('rejects mutation protocol versions other than v2', () => {
    const store = useDocumentSessionStore();
    store.openDocumentResponse(openResponse());
    expect(() => store.applyMutationResponse({
      ...mutation(),
      protocolVersion: 1 as 2,
    })).toThrow('Unsupported editor mutation protocol');
  });
});

function openResponse(): OpenDocumentResponse {
  return {
    document: {
      path: '/tmp/book.xlsx',
      fileName: 'book.xlsx',
      sheets: [
        { name: 'First', extent: { rowCount: 200_000, columnCount: 64 } },
        { name: 'Second', extent: { rowCount: 10, columnCount: 10 } },
      ],
    },
    editorSession: session('0'),
    initialRegion: regionResponse(0, 0, 128, 0, 32, 'A1'),
  };
}

function regionResponse(
  sheetIndex: number,
  rowStart: number,
  rowEnd: number,
  colStart: number,
  colEnd: number,
  display: string
): SheetRegionProjectionResponse {
  return {
    documentId: '1',
    revision: '0',
    region: { sheetIndex, rowStart, rowEnd, colStart, colEnd },
    cells: [{ sheetIndex, row: rowStart, col: colStart, value: { ...blankCell(), display } }],
    metadata: {
      merges: [],
      columnWidths: {},
      rowHeights: {},
      rich: {
        cellFormats: {}, cellStyles: {}, hiddenRows: [], hiddenColumns: [], hyperlinks: {},
        drawings: [], hasMoreDrawings: false, hasStyleMetadata: false,
        hasHyperlinks: false, hasFreezePane: false,
      },
    },
  };
}

function session(revision: `${bigint}`) {
  return {
    documentId: '1' as const,
    revision,
    formulaStatus: readyFormulaStatus(),
    capabilities: defaultWorkbookCapabilities(),
    editorState: {
      canUndo: false,
      canRedo: false,
      isDirty: false,
      history: {
        isTruncated: false,
        undoEntries: 0,
        redoEntries: 0,
        undoEstimatedBytes: 0,
        redoEstimatedBytes: 0,
        maxHistoryBytes: 1,
        maxSingleEntryBytes: 1,
      },
    },
  };
}

function mutation(
  patch?: NonNullable<EditorMutationResponse['patches']>[number],
  sheetExtents = [
    { rowCount: 200_001, columnCount: 64 },
    { rowCount: 10, columnCount: 10 },
  ]
): EditorMutationResponse {
  return {
    protocolVersion: 2,
    documentId: '1',
    revision: '1',
    formulaStatus: readyFormulaStatus(),
    capabilities: defaultWorkbookCapabilities(),
    editorState: session('1').editorState,
    patches: patch ? [patch] : [],
    sheetExtents,
  };
}
