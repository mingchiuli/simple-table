import { describe, expect, it } from 'vitest';

import {
  runtimeDocumentManifest,
  runtimeEditorPatches,
  runtimeRegionProjection,
} from '@/application/documentProjectionProtocol';
import type {
  DocumentManifest,
  EditorPatch,
  SheetRegionProjectionResponse,
} from '@/types/protocol';

describe('document projection protocol', () => {
  it('maps manifests into an independently owned runtime projection', () => {
    const protocol: DocumentManifest = {
      path: '/tmp/book.xlsx',
      fileName: 'book.xlsx',
      sheets: [{
        name: 'Sheet1',
        extent: { rowCount: 2, columnCount: 3 },
        layout: { columnWidths: { 1: 120 } },
      }],
    };

    const runtime = runtimeDocumentManifest(protocol);
    protocol.sheets[0].name = 'Changed';
    protocol.sheets[0].layout.columnWidths![1] = 240;

    expect(runtime.sheets[0].name).toBe('Sheet1');
    expect(runtime.sheets[0].layout.columnWidths).toEqual({ 1: 120 });
  });

  it('normalizes optional region collections and deeply maps cell values', () => {
    const protocol: SheetRegionProjectionResponse = {
      documentId: '1',
      revision: '2',
      region: { sheetIndex: 0, rowStart: 0, rowEnd: 1, colStart: 0, colEnd: 1 },
      cells: [{
        sheetIndex: 0,
        row: 0,
        col: 0,
        value: {
          type: 'cell',
          kind: 'formula',
          raw: null,
          display: '1',
          formula: {
            formula: '=1',
            cachedValue: { type: 'cell', kind: 'number', raw: 1, display: '1' },
          },
        },
      }],
      metadata: {},
    };

    const runtime = runtimeRegionProjection(protocol);
    protocol.cells[0].value.display = 'changed';
    protocol.cells[0].value.formula!.cachedValue.display = 'changed';

    expect(runtime.metadata).toEqual({ merges: [], cellFormats: {}, cellStyles: {} });
    expect(runtime.mergeAnchorCells).toEqual([]);
    expect(runtime.cells[0].value.display).toBe('1');
    expect(runtime.cells[0].value.formula?.cachedValue.display).toBe('1');
  });

  it('maps mutation patch payloads without retaining wire objects', () => {
    const protocol: EditorPatch[] = [{
      type: 'Cells',
      data: {
        changes: [{
          sheetIndex: 0,
          row: 0,
          col: 0,
          value: { type: 'cell', kind: 'text', raw: 'before', display: 'before' },
        }],
      },
    }];

    const runtime = runtimeEditorPatches(protocol)!;
    const wirePatch = protocol[0];
    if (wirePatch.type !== 'Cells') throw new Error('expected cell patch');
    wirePatch.data.changes[0].value.display = 'after';

    expect(runtime[0].type).toBe('Cells');
    if (runtime[0].type === 'Cells') {
      expect(runtime[0].data.changes[0].value.display).toBe('before');
    }
  });
});
