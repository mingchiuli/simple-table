<script setup lang="ts">
import type { CellValue, MergeRange } from '@/types';
import type { SheetRichProjection } from '@/types';
import { usePlatform } from '@/composables/usePlatform';
import { GridViewportShell } from '@/components/table-grid';
import { useGridGeometry } from '@/table-geometry/useGridGeometry';
import { useGridResize } from '@/table-geometry/useGridResize';
import { useGridViewport } from '@/table-geometry/useGridViewport';
import { useTableInteractionController } from '@/table-geometry/useTableInteractionController';
import { createSheetViewportModel } from '@/table-geometry/sheetViewportModel';

const { isTouchDevice } = usePlatform();

const DEFAULT_ROW_HEIGHT = 72;
const DEFAULT_COLUMN_WIDTH = 120;
const HEADER_HEIGHT = 50;
const ROW_HEADER_WIDTH = 60;
const MIN_COLUMN_WIDTH = 56;
const MIN_ROW_HEIGHT = 36;
const OVERSCAN_PX = 240;

const props = defineProps<{
  data: CellValue[][];
  columns: string[];
  sheetIndex: number;
  draftCellValues?: Map<string, string>;
  merges?: MergeRange[];
  selectedCell?: { row: number; col: number } | null;
  autoScroll?: boolean;
  canEditCells?: boolean;
  canResizeRowsColumns?: boolean;
  columnWidths?: Record<number, number>;
  rowHeights?: Record<number, number>;
  layoutResetKey?: number;
  rich?: SheetRichProjection;
}>();

const emit = defineEmits<{
  (e: 'cell-change', rowIndex: number, colIndex: number, value: string): void;
  (e: 'delete-row', index: number): void;
  (e: 'delete-column', index: number): void;
  (e: 'select-cell', rowIndex: number, colIndex: number): void;
  (e: 'cell-editing', rowIndex: number, colIndex: number, value: string): void;
  (e: 'cell-edit-cancel', rowIndex: number, colIndex: number): void;
  (e: 'column-resize', colIndex: number, width: number): void;
  (e: 'row-resize', rowIndex: number, height: number): void;
}>();

const {
  tableSize,
  scrollLeft,
  scrollTop,
  setContainerRef,
  setScrollViewportRef,
  handleViewportScroll,
  scrollCellIntoView,
} = useGridViewport();

function getDraftKey(rowIndex: number, colIndex: number): string {
  return `${props.sheetIndex},${rowIndex},${colIndex}`;
}

function getDraftValue(rowIndex: number, colIndex: number): string | undefined {
  return props.draftCellValues?.get(getDraftKey(rowIndex, colIndex));
}

const viewportModel = computed(() =>
  createSheetViewportModel({
    rows: props.data,
    columns: props.columns,
    merges: props.merges ?? [],
    columnWidths: props.columnWidths,
    rowHeights: props.rowHeights,
    rich: props.rich,
  })
);

const {
  viewportWidth,
  viewportHeight,
  totalColumnsWidth,
  totalRowsHeight,
  visibleRows,
  visibleColumns,
  visibleCellItems,
  visibleMergeCells,
  visibleColumnResizeHandles,
  visibleRowResizeHandles,
  getColumnWidth,
  getRowHeight,
  getRowOffset,
  getColumnOffset,
  getDataColumnOffset,
  setColumnWidth: setPreviewColumnWidth,
  setRowHeight: setPreviewRowHeight,
  resetLayoutFromSource,
  normalizeCellPosition,
} = useGridGeometry({
  sheet: viewportModel,
  selectedCell: computed(() => props.selectedCell),
  tableSize,
  scrollLeft,
  scrollTop,
  rowHeaderWidth: ROW_HEADER_WIDTH,
  headerHeight: HEADER_HEIGHT,
  defaultColumnWidth: DEFAULT_COLUMN_WIDTH,
  defaultRowHeight: DEFAULT_ROW_HEIGHT,
  overscanPx: OVERSCAN_PX,
  getDraftValue,
});

watch(() => props.layoutResetKey, resetLayoutFromSource);

const {
  editingValue,
  isManualClick,
  isEditing,
  getCellKey,
  getDisplayValue,
  handleCellClick,
  handleCellDoubleClick,
  handleInput,
  handleCommit,
  handleCancel,
} = useTableInteractionController({
  data: computed(() => props.data),
  selectedCell: computed(() => props.selectedCell),
  autoScroll: computed(() => props.autoScroll),
  canEditCells: computed(() => props.canEditCells ?? true),
  getDraftValue,
  normalizeCellPosition,
  scrollCellIntoView,
  scrollGeometry: {
    getRowOffset,
    getColumnOffset: getDataColumnOffset,
    getRowHeight,
    getColumnWidth,
    viewportWidth,
    viewportHeight,
  },
  emitSelectCell: (rowIndex, colIndex) => emit('select-cell', rowIndex, colIndex),
  emitEditing: (rowIndex, colIndex, value) => emit('cell-editing', rowIndex, colIndex, value),
  emitChange: (rowIndex, colIndex, value) => emit('cell-change', rowIndex, colIndex, value),
  emitCancel: (rowIndex, colIndex) => emit('cell-edit-cancel', rowIndex, colIndex),
});

const {
  resizingColumn,
  resizingRow,
  resizeLineX,
  resizeLineY,
  startColumnResize,
  startRowResize,
} = useGridResize({
  isTouchDevice,
  canResize: computed(() => props.canResizeRowsColumns ?? true),
  headerHeight: HEADER_HEIGHT,
  minColumnWidth: MIN_COLUMN_WIDTH,
  minRowHeight: MIN_ROW_HEIGHT,
  scrollLeft,
  scrollTop,
  getColumnWidth,
  getRowHeight,
  getColumnOffset,
  getRowOffset,
  setColumnWidth: setPreviewColumnWidth,
  setRowHeight: setPreviewRowHeight,
  commitColumnWidth: (colIndex, width) => emit('column-resize', colIndex, width),
  commitRowHeight: (rowIndex, height) => emit('row-resize', rowIndex, height),
});

function handleDeleteRow(index: number) {
  emit('delete-row', index);
}

function handleDeleteColumn(index: number) {
  emit('delete-column', index);
}

</script>

<template>
  <div
    :ref="setContainerRef"
    class="table-container"
    :style="{
      '--row-header-width': `${ROW_HEADER_WIDTH}px`,
      '--table-header-height': `${HEADER_HEIGHT}px`,
    }"
  >
    <GridViewportShell
      :columns="visibleColumns"
      :rows="visibleRows"
      :scroll-left="scrollLeft"
      :scroll-top="scrollTop"
      :total-columns-width="totalColumnsWidth"
      :total-rows-height="totalRowsHeight"
      :cells="visibleCellItems"
      :merge-cells="visibleMergeCells"
      :selected-cell="selectedCell"
      :is-manual-click="isManualClick"
      :editing-value="editingValue"
      :get-key="getCellKey"
      :get-draft-value="getDraftValue"
      :get-display-value="getDisplayValue"
      :is-editing="isEditing"
      :column-resize-handles="visibleColumnResizeHandles"
      :row-resize-handles="visibleRowResizeHandles"
      :resizing-column="resizingColumn"
      :resizing-row="resizingRow"
      :resize-line-x="resizeLineX"
      :resize-line-y="resizeLineY"
      :is-touch-device="isTouchDevice"
      :set-scroll-viewport-ref="setScrollViewportRef"
      @scroll="handleViewportScroll"
      @delete-row="handleDeleteRow"
      @delete-column="handleDeleteColumn"
      @cell-click="handleCellClick"
      @cell-double-click="handleCellDoubleClick"
      @input="handleInput"
      @commit="handleCommit"
      @cancel="handleCancel"
      @column-resize-start="startColumnResize"
      @row-resize-start="startRowResize"
    />
  </div>
</template>

<style scoped>
.table-container {
  --grid-border-color: var(--el-border-color-lighter);
  width: 100%;
  height: 100%;
  position: relative;
  min-width: 0;
  min-height: 0;
  overflow: hidden;
  background: var(--el-bg-color);
  color: var(--el-text-color-primary);
  font-size: 14px;
  touch-action: pan-x pan-y;
}

:deep(.corner-cell),
:deep(.column-header-cell),
:deep(.row-header-cell) {
  background: var(--el-fill-color-blank);
}

:deep(.corner-cell) {
  position: absolute;
  left: 0;
  top: 0;
  width: var(--row-header-width, 60px);
  height: var(--table-header-height, 50px);
  z-index: 70;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--el-text-color-secondary);
  font-weight: 700;
  box-shadow:
    inset -1px 0 0 var(--grid-border-color),
    inset 0 -1px 0 var(--grid-border-color);
}

:deep(.column-header-viewport) {
  position: absolute;
  left: var(--row-header-width, 60px);
  right: 0;
  top: 0;
  height: var(--table-header-height, 50px);
  z-index: 60;
  overflow: hidden;
  background: var(--el-fill-color-blank);
}

:deep(.column-header-strip),
:deep(.row-header-strip) {
  position: relative;
}

:deep(.column-header-cell) {
  position: absolute;
  top: 0;
  height: var(--table-header-height, 50px);
  box-sizing: border-box;
  box-shadow:
    inset -1px 0 0 var(--grid-border-color),
    inset 0 -1px 0 var(--grid-border-color);
}

:deep(.row-header-viewport) {
  position: absolute;
  left: 0;
  top: var(--table-header-height, 50px);
  bottom: 0;
  width: var(--row-header-width, 60px);
  z-index: 60;
  overflow: hidden;
  background: var(--el-fill-color-blank);
}

:deep(.row-header-cell) {
  position: absolute;
  left: 0;
  width: var(--row-header-width, 60px);
  box-sizing: border-box;
  box-shadow:
    inset -1px 0 0 var(--grid-border-color),
    inset 0 -1px 0 var(--grid-border-color);
}

:deep(.data-cell),
:deep(.merge-cell) {
  position: absolute;
  box-sizing: border-box;
  background: var(--el-bg-color);
  overflow: hidden;
}

:deep(.data-cell) {
  z-index: 1;
  box-shadow:
    inset -1px 0 0 var(--grid-border-color),
    inset 0 -1px 0 var(--grid-border-color);
}

:deep(.merge-cell) {
  z-index: 3;
  box-shadow: inset 0 0 0 1px var(--grid-border-color);
}

:deep(.data-cell.is-selected)::after,
:deep(.merge-cell.is-selected)::after {
  content: "";
  position: absolute;
  inset: -1px;
  border: 2px solid var(--el-color-primary);
  pointer-events: none;
  z-index: 5;
}

:deep(.merge-cell.is-editing) {
  z-index: 4;
}

:deep(.resize-overlay) {
  position: absolute;
  inset: 0;
  z-index: 90;
  pointer-events: none;
}

:deep(.column-resize-handle) {
  position: absolute;
  top: 0;
  bottom: 0;
  width: 10px;
  transform: translateX(-5px);
  cursor: col-resize;
  pointer-events: auto;
}

:deep(.column-resize-handle)::after {
  content: "";
  position: absolute;
  top: 0;
  bottom: 0;
  left: 4px;
  width: 2px;
  background: transparent;
}

:deep(.column-resize-handle:hover)::after,
:deep(.column-resize-handle.is-active)::after {
  background: var(--el-color-primary);
}

:deep(.row-resize-handle) {
  position: absolute;
  left: 0;
  right: 0;
  height: 10px;
  transform: translateY(-5px);
  cursor: row-resize;
  pointer-events: auto;
  touch-action: none;
}

:deep(.row-resize-handle)::after {
  content: "";
  position: absolute;
  left: 0;
  right: 0;
  top: 4px;
  height: 2px;
  background: transparent;
}

:deep(.row-resize-handle:hover)::after,
:deep(.row-resize-handle.is-active)::after {
  background: var(--el-color-primary);
}

:deep(.resize-line) {
  position: absolute;
  top: 0;
  bottom: 0;
  width: 2px;
  background-color: var(--el-color-primary);
  z-index: 100;
  pointer-events: none;
  transform: translateX(-1px);
}

:deep(.resize-line.horizontal) {
  left: 0;
  right: 0;
  bottom: auto;
  width: auto;
  height: 2px;
  transform: translateY(-1px);
}

@media (pointer: coarse) and (hover: none) {
  .table-container {
    font-size: 16px;
  }

  :deep(.column-resize-handle) {
    width: 18px;
    transform: translateX(-9px);
  }

  :deep(.column-resize-handle)::after {
    left: 8px;
  }

  :deep(.row-resize-handle) {
    height: 18px;
    transform: translateY(-9px);
  }

  :deep(.row-resize-handle)::after {
    top: 8px;
  }
}
</style>
