<script setup lang="ts">
import type { ColumnResizeHandle, RowResizeHandle } from "@/table-geometry/gridGeometry";

defineProps<{
  columnHandles: ColumnResizeHandle[];
  rowHandles: RowResizeHandle[];
  resizingColumn: number | null;
  resizingRow: number | null;
  resizeLineX: number;
  resizeLineY: number;
  isTouchDevice: boolean;
}>();

const emit = defineEmits<{
  (e: "column-resize-start", event: MouseEvent | TouchEvent, colIndex: number, boundaryX: number): void;
  (e: "row-resize-start", event: MouseEvent | TouchEvent, rowIndex: number, boundaryY: number): void;
}>();
</script>

<template>
  <div class="resize-overlay" aria-hidden="true">
    <div
      v-for="handle in columnHandles"
      :key="handle.colIndex"
      class="column-resize-handle"
      :class="{ 'is-active': resizingColumn === handle.colIndex }"
      :style="{ left: `${handle.left}px` }"
      @mousedown.stop="emit('column-resize-start', $event, handle.colIndex, handle.left)"
      @touchstart.stop="(event: TouchEvent) => isTouchDevice && emit('column-resize-start', event, handle.colIndex, handle.left)"
    />
    <div
      v-for="handle in rowHandles"
      :key="handle.rowIndex"
      class="row-resize-handle"
      :class="{ 'is-active': resizingRow === handle.rowIndex }"
      :style="{ top: `${handle.top}px` }"
      @mousedown.stop="emit('row-resize-start', $event, handle.rowIndex, handle.top)"
      @touchstart.stop="(event: TouchEvent) => isTouchDevice && emit('row-resize-start', event, handle.rowIndex, handle.top)"
    />
  </div>

  <div
    v-if="resizingColumn !== null"
    class="resize-line"
    :style="{ left: `${resizeLineX}px` }"
  />
  <div
    v-if="resizingRow !== null"
    class="resize-line horizontal"
    :style="{ top: `${resizeLineY}px` }"
  />
</template>
