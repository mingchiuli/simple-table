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

const offsets = geometry.buildOffsets(3, (index) => [40, 60, 80][index]);
assert.deepEqual(offsets, [0, 40, 100, 180]);
assert.equal(geometry.offsetAt(offsets, 2, 0), 100);
assert.equal(geometry.spanSize(offsets, 0, 2, 0), 180);

assert.deepEqual(
  geometry.collectVisibleItems(offsets, 3, 45, 50, 0),
  [
    { index: 1, top: 40, height: 60 },
  ]
);
assert.deepEqual(
  geometry.collectVisibleItems(offsets, 3, 40, 60, 0),
  [
    { index: 0, top: 0, height: 40 },
    { index: 1, top: 40, height: 60 },
    { index: 2, top: 100, height: 80 },
  ],
  'visible collection keeps boundary-touching rows so grid lines do not disappear'
);
assert.deepEqual(
  geometry.collectVisibleItems(offsets, 3, 45, 50, 20),
  [
    { index: 0, top: 0, height: 40 },
    { index: 1, top: 40, height: 60 },
    { index: 2, top: 100, height: 80 },
  ]
);

assert.deepEqual(
  geometry.collectColumnResizeHandles(3, 60, 20, 180, (index) => [100, 120, 80][index]),
  [
    { colIndex: 0, left: 140 },
  ]
);
assert.deepEqual(
  geometry.collectColumnResizeHandles(3, 60, 0, 260, (index) => [100, 120, 80][index]),
  [
    { colIndex: 0, left: 160 },
    { colIndex: 1, left: 280 },
  ].filter((handle) => handle.left <= 260),
  'column resize handles are placed on rendered column boundaries'
);
assert.deepEqual(
  geometry.collectRowResizeHandles(3, 50, 10, 160, (index) => [40, 60, 80][index]),
  [
    { rowIndex: 0, top: 80 },
    { rowIndex: 1, top: 140 },
  ]
);
assert.deepEqual(
  geometry.collectRowResizeHandles(3, 50, 0, 150, (index) => [40, 60, 80][index]),
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
assert.equal(
  geometry.areNumberRecordsEqual({ 1: 120 }, { 1: 121 }),
  false
);

console.log('grid geometry tests passed');
