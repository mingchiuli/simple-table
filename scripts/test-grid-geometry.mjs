import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import ts from 'typescript';

const source = await readFile(new URL('../src/table-geometry/gridGeometry.ts', import.meta.url), 'utf8');
const { outputText } = ts.transpileModule(source, {
  compilerOptions: {
    module: ts.ModuleKind.ES2022,
    target: ts.ScriptTarget.ES2022,
  },
});

const geometry = await import(`data:text/javascript;charset=utf-8,${encodeURIComponent(outputText)}`);

function axis(sizes) {
  return new geometry.SparseAxisGeometry(
    sizes.length,
    0,
    Object.fromEntries(sizes.map((size, index) => [index, size]))
  );
}

const rows = axis([40, 60, 80]);
assert.equal(rows.offsetAt(2), 100);
assert.equal(rows.totalSize(), 180);
assert.equal(rows.offsetAt(3) - rows.offsetAt(0), 180);

assert.deepEqual(
  geometry.collectVisibleItems(rows, 45, 50, 0),
  [{ index: 1, top: 40, height: 60 }]
);
assert.deepEqual(
  geometry.collectVisibleItems(rows, 40, 60, 0),
  [
    { index: 0, top: 0, height: 40 },
    { index: 1, top: 40, height: 60 },
    { index: 2, top: 100, height: 80 },
  ],
  'visible collection keeps boundary-touching rows so grid lines do not disappear'
);
assert.deepEqual(
  geometry.collectVisibleItems(rows, 45, 50, 20),
  [
    { index: 0, top: 0, height: 40 },
    { index: 1, top: 40, height: 60 },
    { index: 2, top: 100, height: 80 },
  ]
);

assert.deepEqual(
  geometry.collectColumnResizeHandles(axis([100, 120, 80]), 60, 20, 180),
  [{ colIndex: 0, left: 140 }]
);
assert.deepEqual(
  geometry.collectColumnResizeHandles(axis([100, 120, 80]), 60, 0, 260),
  [
    { colIndex: 0, left: 160 },
    { colIndex: 1, left: 280 },
  ].filter((handle) => handle.left <= 260),
  'column resize handles are placed on rendered column boundaries'
);
assert.deepEqual(
  geometry.collectRowResizeHandles(rows, 50, 10, 160),
  [
    { rowIndex: 0, top: 80 },
    { rowIndex: 1, top: 140 },
  ]
);
assert.deepEqual(
  geometry.collectRowResizeHandles(rows, 50, 0, 150),
  [
    { rowIndex: 0, top: 90 },
    { rowIndex: 1, top: 150 },
  ],
  'row resize handles are placed on rendered row boundaries'
);

assert.equal(
  geometry.areNumberRecordsEqual({ 1: 120, 3: 180 }, { 3: 180, 1: 120 }),
  true
);
assert.equal(geometry.areNumberRecordsEqual({ 1: 120 }, { 1: 121 }), false);

const mergedScenario = {
  columnWidths: [100, 150, 90, 120],
  rowHeights: [48, 72, 64, 80],
  rowHeaderWidth: 60,
  headerHeight: 50,
};
const mergedColumns = axis(mergedScenario.columnWidths);
const mergedRows = axis(mergedScenario.rowHeights);
const mergedCell = {
  left: mergedColumns.offsetAt(1),
  top: mergedRows.offsetAt(1),
  width: mergedColumns.offsetAt(3) - mergedColumns.offsetAt(1),
  height: mergedRows.offsetAt(3) - mergedRows.offsetAt(1),
};
assert.deepEqual(
  mergedCell,
  { left: 100, top: 48, width: 240, height: 136 },
  'merged cells use accumulated row and column geometry instead of first-cell size'
);
assert.deepEqual(
  geometry.collectColumnResizeHandles(
    mergedColumns,
    mergedScenario.rowHeaderWidth,
    0,
    420
  ),
  [
    { colIndex: 0, left: 160 },
    { colIndex: 1, left: 310 },
    { colIndex: 2, left: 400 },
  ],
  'column resize handles stay on visible grid boundaries with merged cells present'
);
assert.deepEqual(
  geometry.collectRowResizeHandles(mergedRows, mergedScenario.headerHeight, 0, 300),
  [
    { rowIndex: 0, top: 98 },
    { rowIndex: 1, top: 170 },
    { rowIndex: 2, top: 234 },
  ],
  'row resize handles stay on visible grid boundaries with merged cells present'
);

const largeRows = new geometry.SparseAxisGeometry(250_000, 72, { 200_000: 144 });
const deepVisible = geometry.collectVisibleItems(largeRows, 14_400_001, 720, 0);
assert.equal(deepVisible[0].index, 200_000);
assert.ok(deepVisible.length < 20);

console.log('grid geometry tests passed');
