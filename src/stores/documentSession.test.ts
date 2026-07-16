import { beforeEach, describe, expect, it } from 'vitest';
import { createPinia, setActivePinia } from 'pinia';
import { useDocumentSessionStore } from '@/stores/documentSession';
import { defaultWorkbookCapabilities, readyFormulaStatus } from '@/types';
import type {
  EditorMutationResponse,
  OpenDocumentResponse,
  SheetRegionProjectionResponse,
} from '@/types';
import { isCellLoaded, loadedSheetMetadata, sheetCell } from '@/stores/documentProjection';
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

  it('charges the initial region against the resident byte budget', () => {
    const store = useDocumentSessionStore();
    const response = openResponse();
    response.initialRegion!.estimatedBytes = 16 * 1024 * 1024 + 1;

    store.openDocumentResponse(response);

    expect(store.loadedSheet(0)?.blocks).toHaveLength(0);
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

  it('rejects a region block larger than the backend response contract', async () => {
    const store = useDocumentSessionStore();
    store.openDocumentResponse(openResponse());
    const response = regionResponse(0, 128, 256, 0, 32, 'oversized');
    response.estimatedBytes = 16 * 1024 * 1024 + 1;

    expect(await store.ensureSheetRegionLoaded(response.region, async () => response)).toBe(false);
    expect(store.loadedSheet(0)?.blocks).toHaveLength(1);
  });

  it('subdivides oversized region responses and reuses the combined coverage', async () => {
    const store = useDocumentSessionStore();
    store.openDocumentResponse(openResponse());
    const requested = regionResponse(0, 128, 256, 0, 32, 'tile').region;
    let requests = 0;
    const fetch = async (_context: unknown, region: typeof requested) => {
      requests += 1;
      if (region.rowEnd - region.rowStart > 64) {
        throw {
          code: 'region_response_too_large',
          message: 'region response exceeds byte limit',
        };
      }
      return regionResponse(
        region.sheetIndex,
        region.rowStart,
        region.rowEnd,
        region.colStart,
        region.colEnd,
        `row-${region.rowStart}`
      );
    };

    expect(await store.ensureSheetRegionLoaded(requested, fetch)).toBe(true);
    expect(requests).toBe(3);
    expect(sheetCell(store.data?.sheets[0], 128, 0)?.display).toBe('row-128');
    expect(sheetCell(store.data?.sheets[0], 192, 0)?.display).toBe('row-192');

    expect(await store.ensureSheetRegionLoaded(requested, fetch)).toBe(true);
    expect(requests).toBe(3);
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

  it('keeps sheet layout stable when cell blocks are evicted', async () => {
    const store = useDocumentSessionStore();
    const opened = openResponse();
    opened.document.sheets[0].layout = {
      columnWidths: { 40: 240 },
      rowHeights: { 1000: 96 },
    };
    store.openDocumentResponse(opened);
    for (let tile = 1; tile <= 10; tile += 1) {
      const response = regionResponse(0, tile * 128, (tile + 1) * 128, 0, 32, `${tile}`);
      await store.ensureSheetRegionLoaded(response.region, async () => response);
    }

    const sheet = store.loadedSheet(0);
    expect(sheet).not.toBeNull();
    expect(loadedSheetMetadata(sheet!).rowHeights).toEqual({ 1000: 96 });
    expect(loadedSheetMetadata(sheet!).columnWidths).toEqual({ 40: 240 });
  });

  it('reads merge anchors that live outside the loaded tile', async () => {
    const store = useDocumentSessionStore();
    store.openDocumentResponse(openResponse());
    const response = regionResponse(0, 128, 256, 0, 32, 'tail');
    response.metadata.merges = [{ startRow: 0, startCol: 0, endRow: 140, endCol: 0 }];
    response.mergeAnchorCells = [{
      sheetIndex: 0,
      row: 0,
      col: 0,
      value: { ...blankCell(), display: 'anchor' },
    }];
    store.loadedSheet(0)!.blocks = [];

    expect(await store.ensureSheetRegionLoaded(response.region, async () => response)).toBe(true);
    expect(sheetCell(store.data?.sheets[0], 0, 0)?.display).toBe('anchor');
    expect(isCellLoaded(store.data?.sheets[0], 0, 0)).toBe(true);
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

  it('applies incremental layout patches without invalidating cell blocks', () => {
    const store = useDocumentSessionStore();
    store.openDocumentResponse(openResponse());
    store.applyMutationResponse(mutation({
      type: 'Layout',
      data: {
        patch: {
          sheetIndex: 0,
          columnWidths: { 4: 180 },
          rowHeights: { 7: 44 },
        },
      },
    }));

    expect(store.loadedSheet(0)?.blocks).toHaveLength(1);
    expect(store.data?.sheets[0].layout).toEqual({
      columnWidths: { 4: 180 },
      rowHeights: { 7: 44 },
    });
  });

  it('applies layout patches to unloaded sheets', () => {
    const store = useDocumentSessionStore();
    store.openDocumentResponse(openResponse());
    store.applyMutationResponse(mutation({
      type: 'Layout',
      data: {
        patch: {
          sheetIndex: 1,
          columnWidths: { 2: 96 },
        },
      },
    }));

    expect(store.data?.sheets[1]).toMatchObject({
      state: 'unloaded',
      layout: { columnWidths: { 2: 96 }, rowHeights: {} },
    });
  });

  it('replaces layout from the server after a structural patch', () => {
    const store = useDocumentSessionStore();
    store.openDocumentResponse(openResponse());
    store.applyMutationResponse({
      ...mutation({
        type: 'RowInserted',
        data: { patch: { sheetIndex: 0, rowIndex: 2, count: 1 } },
      }),
      sheetLayouts: [{
        sheetIndex: 0,
        layout: { columnWidths: { 3: 120 }, rowHeights: { 8: 36 } },
      }],
    });

    expect(store.data?.sheets[0].layout).toEqual({
      columnWidths: { 3: 120 },
      rowHeights: { 8: 36 },
    });
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
          sheet: {
            name: 'Inserted',
            extent: { rowCount: 0, columnCount: 0 },
            layout: { columnWidths: {}, rowHeights: {} },
          },
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

  it('rejects mutation protocol versions other than v3', () => {
    const store = useDocumentSessionStore();
    store.openDocumentResponse(openResponse());
    expect(() => store.applyMutationResponse({
      ...mutation(),
      protocolVersion: 1 as 3,
    })).toThrow('Unsupported editor mutation protocol');
  });
});

function openResponse(): OpenDocumentResponse {
  return {
    document: {
      path: '/tmp/book.xlsx',
      fileName: 'book.xlsx',
      sheets: [
        {
          name: 'First',
          extent: { rowCount: 200_000, columnCount: 64 },
          layout: { columnWidths: {}, rowHeights: {} },
        },
        {
          name: 'Second',
          extent: { rowCount: 10, columnCount: 10 },
          layout: { columnWidths: {}, rowHeights: {} },
        },
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
    mergeAnchorCells: [],
    metadata: {
      merges: [],
      cellFormats: {},
      cellStyles: {},
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
    protocolVersion: 3,
    documentId: '1',
    revision: '1',
    formulaStatus: readyFormulaStatus(),
    capabilities: defaultWorkbookCapabilities(),
    editorState: session('1').editorState,
    patches: patch ? [patch] : [],
    sheetExtents,
  };
}
