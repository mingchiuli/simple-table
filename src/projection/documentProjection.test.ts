import { describe, expect, it } from 'vitest';
import { applyProjectionPatches } from '@/projection/documentProjection';
import type { CellValue, DocumentProjection, EditorPatch, SheetRegionBlock } from '@/types';
import { defaultRichProjection } from '@/types';
import { blankCell } from '@/utils/cellValue';

describe('documentProjection patch reduction', () => {
  it('copies each affected cell index once for a batch', () => {
    const source: Record<string, CellValue> = {};
    const changes: Extract<EditorPatch, { type: 'Cells' }>['data']['changes'] = [];
    for (let col = 0; col < 32; col += 1) {
      source[`0:${col}`] = { ...blankCell(), display: `old-${col}` };
      changes.push({
        sheetIndex: 0,
        row: 0,
        col,
        value: { ...blankCell(), display: `new-${col}` },
      });
    }
    let enumerations = 0;
    const cells = new Proxy(source, {
      ownKeys(target) {
        enumerations += 1;
        return Reflect.ownKeys(target);
      },
    });
    const untouched = block(0, 128, 256, {});
    const data = projection([block(0, 0, 128, cells), untouched]);

    const result = applyProjectionPatches(data, [{ type: 'Cells', data: { changes } }]);
    const updated = result.data!.sheets[0];

    expect(enumerations).toBe(1);
    expect(updated.state).toBe('loaded');
    if (updated.state !== 'loaded') throw new Error('Expected loaded sheet');
    expect(updated.blocks[0].cells['0:31']?.display).toBe('new-31');
    expect(updated.blocks[1]).toBe(untouched);
  });

  it('preserves the complete projection for status-only responses', () => {
    const data = projection([block(0, 0, 128, {})]);

    const result = applyProjectionPatches(data, [], [
      { rowCount: 256, columnCount: 32 },
    ]);

    expect(result.data).toBe(data);
  });

  it('updates only sheets whose extent changed', () => {
    const data = projection([block(0, 0, 128, {})]);
    data.sheets.push({
      state: 'unloaded',
      name: 'Second',
      extent: { rowCount: 10, columnCount: 10 },
      layout: { columnWidths: {}, rowHeights: {} },
    });
    const first = data.sheets[0];
    const second = data.sheets[1];

    const result = applyProjectionPatches(data, [], [
      { rowCount: 256, columnCount: 32 },
      { rowCount: 11, columnCount: 10 },
    ]);

    expect(result.data?.sheets[0]).toBe(first);
    expect(result.data?.sheets[1]).not.toBe(second);
    expect(result.data?.sheets[1].extent.rowCount).toBe(11);
  });

  it('does not mutate the input projection when invalidating a sheet', () => {
    const data = projection([block(0, 0, 128, {})]);
    const originalSheets = data.sheets;
    const originalSheet = data.sheets[0];

    const result = applyProjectionPatches(data, [{
      type: 'SheetInvalidated',
      data: { patch: { sheetIndex: 0 } },
    }]);

    expect(data.sheets).toBe(originalSheets);
    expect(data.sheets[0]).toBe(originalSheet);
    expect(data.sheets[0].state).toBe('loaded');
    expect(result.data?.sheets[0].state).toBe('loaded');
    if (result.data?.sheets[0].state !== 'loaded') throw new Error('Expected loaded sheet');
    expect(result.data.sheets[0].blocks).toHaveLength(0);
  });
});

function projection(blocks: SheetRegionBlock[]): DocumentProjection {
  return {
    path: '/tmp/book.xlsx',
    fileName: 'book.xlsx',
    sheets: [{
      state: 'loaded',
      name: 'First',
      extent: { rowCount: 256, columnCount: 32 },
      layout: { columnWidths: {}, rowHeights: {} },
      blocks,
      metadata: {
        merges: [],
        rich: defaultRichProjection(),
      },
    }],
  };
}

function block(
  sheetIndex: number,
  rowStart: number,
  rowEnd: number,
  cells: Record<string, CellValue>
): SheetRegionBlock {
  return {
    key: `${sheetIndex}:${rowStart}:${rowEnd}:0:32`,
    region: { sheetIndex, rowStart, rowEnd, colStart: 0, colEnd: 32 },
    cells,
    mergeAnchorCells: {},
    metadata: { merges: [], cellFormats: {}, cellStyles: {} },
    wireBytes: 1,
    residentBytes: 1,
  };
}
