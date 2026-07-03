<script setup lang="ts">
import { GridCellsLayer, GridHeaders, MergeCellsLayer, ResizeLayer } from "@/components/table-grid";
import type { ColumnResizeHandle, GridItem, RowResizeHandle } from "@/table-geometry/gridGeometry";
import type { CellItem, ColumnItem, MergeOverlayCell } from "@/table-geometry/useGridGeometry";
import type { CellValue } from "@/types";

defineProps<{
  columns: ColumnItem[];
  rows: GridItem[];
  totalColumnsWidth: number;
  totalRowsHeight: number;
  scrollLeft: number;
  scrollTop: number;
  cells: CellItem[];
  mergeCells: MergeOverlayCell[];
  selectedCell?: { row: number; col: number } | null;
  isManualClick: boolean;
  editingValue: Record<string, string>;
  getKey: (rowIndex: number, colIndex: number) => string;
  getDraftValue: (rowIndex: number, colIndex: number) => string | undefined;
  getDisplayValue: (rowIndex: number, colIndex: number, cellValue: CellValue | undefined) => string;
  isEditing: (rowIndex: number, colIndex: number) => boolean;
  columnResizeHandles: ColumnResizeHandle[];
  rowResizeHandles: RowResizeHandle[];
  resizingColumn: number | null;
  resizingRow: number | null;
  resizeLineX: number;
  resizeLineY: number;
  isTouchDevice: boolean;
  setScrollViewportRef: (element: unknown) => void;
}>();

const emit = defineEmits<{
  (e: "scroll"): void;
  (e: "delete-row", index: number): void;
  (e: "delete-column", index: number): void;
  (e: "cell-click", rowIndex: number, colIndex: number): void;
  (e: "cell-double-click", rowIndex: number, colIndex: number): void;
  (e: "input", rowIndex: number, colIndex: number, value: string): void;
  (e: "commit", rowIndex: number, colIndex: number, value: string): void;
  (e: "cancel", rowIndex: number, colIndex: number): void;
  (e: "column-resize-start", event: MouseEvent | TouchEvent, colIndex: number, boundaryX: number): void;
  (e: "row-resize-start", event: MouseEvent | TouchEvent, rowIndex: number, boundaryY: number): void;
}>();
</script>

<template>
  <GridHeaders
    :columns="columns"
    :rows="rows"
    :scroll-left="scrollLeft"
    :scroll-top="scrollTop"
    :total-columns-width="totalColumnsWidth"
    :total-rows-height="totalRowsHeight"
    @delete-row="emit('delete-row', $event)"
    @delete-column="emit('delete-column', $event)"
  />

  <div :ref="setScrollViewportRef" class="data-viewport" @scroll="emit('scroll')">
    <div
      class="data-scroll-content"
      :style="{
        width: `${totalColumnsWidth}px`,
        height: `${totalRowsHeight}px`,
      }"
    >
      <GridCellsLayer
        :cells="cells"
        :selected-cell="selectedCell"
        :is-manual-click="isManualClick"
        :editing-value="editingValue"
        :get-key="getKey"
        :get-draft-value="getDraftValue"
        :get-display-value="getDisplayValue"
        :is-editing="isEditing"
        @cell-click="(rowIndex, colIndex) => emit('cell-click', rowIndex, colIndex)"
        @cell-double-click="(rowIndex, colIndex) => emit('cell-double-click', rowIndex, colIndex)"
        @input="(rowIndex, colIndex, value) => emit('input', rowIndex, colIndex, value)"
        @commit="(rowIndex, colIndex, value) => emit('commit', rowIndex, colIndex, value)"
        @cancel="(rowIndex, colIndex) => emit('cancel', rowIndex, colIndex)"
      />

      <MergeCellsLayer
        :cells="mergeCells"
        :is-manual-click="isManualClick"
        :editing-value="editingValue"
        :get-key="getKey"
        :get-display-value="getDisplayValue"
        :is-editing="isEditing"
        @cell-click="(rowIndex, colIndex) => emit('cell-click', rowIndex, colIndex)"
        @cell-double-click="(rowIndex, colIndex) => emit('cell-double-click', rowIndex, colIndex)"
        @input="(rowIndex, colIndex, value) => emit('input', rowIndex, colIndex, value)"
        @commit="(rowIndex, colIndex, value) => emit('commit', rowIndex, colIndex, value)"
        @cancel="(rowIndex, colIndex) => emit('cancel', rowIndex, colIndex)"
      />
    </div>
  </div>

  <ResizeLayer
    :column-handles="columnResizeHandles"
    :row-handles="rowResizeHandles"
    :resizing-column="resizingColumn"
    :resizing-row="resizingRow"
    :resize-line-x="resizeLineX"
    :resize-line-y="resizeLineY"
    :is-touch-device="isTouchDevice"
    @column-resize-start="(event, colIndex, boundaryX) => emit('column-resize-start', event, colIndex, boundaryX)"
    @row-resize-start="(event, rowIndex, boundaryY) => emit('row-resize-start', event, rowIndex, boundaryY)"
  />
</template>

<style scoped>
.data-viewport {
  position: absolute;
  left: var(--row-header-width, 60px);
  right: 0;
  top: var(--table-header-height, 50px);
  bottom: 0;
  z-index: 10;
  overflow: auto;
  background: var(--el-bg-color);
  overscroll-behavior: contain;
}

.data-scroll-content {
  position: relative;
  min-width: 100%;
  min-height: 100%;
}

@media (pointer: coarse) and (hover: none) {
  .data-viewport {
    -webkit-overflow-scrolling: touch;
  }
}
</style>
