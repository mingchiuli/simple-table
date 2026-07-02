<script setup lang="ts">
import type { CellFormatProjection, CellStyleProjection, CellValue } from "@/types";
import { CellView, EditableCell } from "@/components/cell";

type MergeCell = {
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
  draftValue?: string;
  selected: boolean;
};

defineProps<{
  cells: MergeCell[];
  isManualClick: boolean;
  editingValue: Record<string, string>;
  getKey: (rowIndex: number, colIndex: number) => string;
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
    v-for="mergeCell in cells"
    :key="mergeCell.key"
    class="merge-cell"
    :class="{ 'is-selected': mergeCell.selected, 'is-editing': isEditing(mergeCell.rowIndex, mergeCell.colIndex) }"
    :style="{
      left: `${mergeCell.left}px`,
      top: `${mergeCell.top}px`,
      width: `${mergeCell.width}px`,
      height: `${mergeCell.height}px`,
    }"
    @click="emit('cell-click', mergeCell.rowIndex, mergeCell.colIndex)"
    @dblclick="emit('cell-double-click', mergeCell.rowIndex, mergeCell.colIndex)"
  >
    <CellView
      v-if="!isEditing(mergeCell.rowIndex, mergeCell.colIndex)"
      :value="mergeCell.value"
      :format="mergeCell.format"
      :cell-style="mergeCell.style"
      :draft-value="mergeCell.draftValue"
      :selected="false"
      :row-height="mergeCell.height"
    />
    <EditableCell
      v-else
      :auto-focus="isManualClick"
      :min-height="mergeCell.height"
      :model-value="editingValue[getKey(mergeCell.rowIndex, mergeCell.colIndex)] ?? getDisplayValue(mergeCell.rowIndex, mergeCell.colIndex, mergeCell.value)"
      @update:model-value="(value: string) => emit('input', mergeCell.rowIndex, mergeCell.colIndex, value)"
      @commit="emit('commit', mergeCell.rowIndex, mergeCell.colIndex, editingValue[getKey(mergeCell.rowIndex, mergeCell.colIndex)] ?? getDisplayValue(mergeCell.rowIndex, mergeCell.colIndex, mergeCell.value))"
      @cancel="emit('cancel', mergeCell.rowIndex, mergeCell.colIndex)"
    />
  </div>
</template>
