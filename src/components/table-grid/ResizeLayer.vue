<script setup lang="ts">
import type { ColumnResizeHandle } from "@/table-geometry/gridGeometry";

defineProps<{
  handles: ColumnResizeHandle[];
  resizingColumn: number | null;
  resizingRow: number | null;
  resizeLineX: number;
  resizeLineY: number;
  isTouchDevice: boolean;
}>();

const emit = defineEmits<{
  (e: "column-resize-start", event: MouseEvent | TouchEvent, colIndex: number, boundaryX: number): void;
}>();
</script>

<template>
  <div class="resize-overlay" aria-hidden="true">
    <div
      v-for="handle in handles"
      :key="handle.colIndex"
      class="column-resize-handle"
      :class="{ 'is-active': resizingColumn === handle.colIndex }"
      :style="{ left: `${handle.left}px` }"
      @mousedown.stop="emit('column-resize-start', $event, handle.colIndex, handle.left)"
      @touchstart.stop="(event: TouchEvent) => isTouchDevice && emit('column-resize-start', event, handle.colIndex, handle.left)"
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
