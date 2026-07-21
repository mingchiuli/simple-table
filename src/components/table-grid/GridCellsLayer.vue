<script setup lang="ts">
import type { CellFormatProjection, CellStyleProjection, CellValue } from "@/types";
import { CellView, EditableCell } from "@/components/cell";

type CellItem = {
  key: string;
  rowIndex: number;
  colIndex: number;
  top: number;
  left: number;
  width: number;
  height: number;
  value: CellValue | undefined;
  format?: CellFormatProjection;
  style?: CellStyleProjection;
};

defineProps<{
  cells: CellItem[];
  selectedCell?: { row: number; col: number } | null;
  isManualClick: boolean;
  editingValue: Record<string, string>;
  getKey: (rowIndex: number, colIndex: number) => string;
  getDraftValue: (rowIndex: number, colIndex: number) => string | undefined;
  getDisplayValue: (rowIndex: number, colIndex: number, cellValue: CellValue | undefined) => string;
  isEditing: (rowIndex: number, colIndex: number) => boolean;
}>();

const emit = defineEmits<{
  (e: "cell-click", rowIndex: number, colIndex: number): void;
  (e: "cell-double-click", rowIndex: number, colIndex: number): void;
  (e: "input", rowIndex: number, colIndex: number, value: string): void;
  (e: "commit", rowIndex: number, colIndex: number, value: string): void;
  (e: "cancel", rowIndex: number, colIndex: number): void;
}>();
</script>

<template>
  <div
    v-for="cell in cells"
    :key="cell.key"
    class="data-cell"
    :class="{ 'is-selected': selectedCell?.row === cell.rowIndex && selectedCell?.col === cell.colIndex }"
    :style="{
      left: `${cell.left}px`,
      top: `${cell.top}px`,
      width: `${cell.width}px`,
      height: `${cell.height}px`,
    }"
    @click="emit('cell-click', cell.rowIndex, cell.colIndex)"
    @dblclick="emit('cell-double-click', cell.rowIndex, cell.colIndex)"
  >
    <CellView
      v-if="!isEditing(cell.rowIndex, cell.colIndex)"
      :value="cell.value"
      :format="cell.format"
      :cell-style="cell.style"
      :draft-value="getDraftValue(cell.rowIndex, cell.colIndex)"
      :selected="false"
    />
    <EditableCell
      v-else
      :auto-focus="isManualClick"
      :model-value="editingValue[getKey(cell.rowIndex, cell.colIndex)] ?? getDisplayValue(cell.rowIndex, cell.colIndex, cell.value)"
      @update:model-value="(value: string) => emit('input', cell.rowIndex, cell.colIndex, value)"
      @commit="emit('commit', cell.rowIndex, cell.colIndex, editingValue[getKey(cell.rowIndex, cell.colIndex)] ?? getDisplayValue(cell.rowIndex, cell.colIndex, cell.value))"
      @cancel="emit('cancel', cell.rowIndex, cell.colIndex)"
    />
  </div>
</template>
