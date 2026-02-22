<script setup lang="ts">
import { ref } from 'vue';
import type { CellValue } from '@/types';

const props = defineProps<{
  data: CellValue[][];
  columns: string[];
}>();

const emit = defineEmits<{
  (e: 'cell-change', rowIndex: number, colIndex: number, value: string): void;
  (e: 'delete-row', index: number): void;
}>();

// 本地编辑状态
const editingValue = ref<Record<string, string>>({});

function getCellValue(cell: CellValue): string {
  if (cell === null || cell === undefined) return '';
  return String(cell);
}

function getKey(rowIndex: number, colIndex: number): string {
  return `${rowIndex}-${colIndex}`;
}

function handleInput(rowIndex: number, colIndex: number, value: string) {
  const key = getKey(rowIndex, colIndex);
  editingValue.value[key] = value;
}

function handleBlur(rowIndex: number, colIndex: number, value: string) {
  const key = getKey(rowIndex, colIndex);
  const originalValue = getCellValue(props.data[rowIndex]?.[colIndex]);

  if (value !== originalValue) {
    emit('cell-change', rowIndex, colIndex, value);
  }

  // 清空本地状态
  delete editingValue.value[key];
}

function handleDeleteRow(index: number) {
  emit('delete-row', index);
}

function getDisplayValue(rowIndex: number, colIndex: number, cellValue: CellValue): string {
  const key = getKey(rowIndex, colIndex);
  if (editingValue.value[key] !== undefined) {
    return editingValue.value[key];
  }
  return getCellValue(cellValue);
}
</script>

<template>
  <el-table
    :data="props.data"
    border
    stripe
    style="width: 100%; height: 100%"
  >
    <el-table-column type="index" width="60" fixed="left">
      <template #header>
        <span>#</span>
      </template>
      <template #default="{ $index }">
        <div class="row-index">
          <span>{{ $index + 1 }}</span>
          <el-button
            type="danger"
            size="small"
            text
            @click.stop="handleDeleteRow($index)"
          >
            ×
          </el-button>
        </div>
      </template>
    </el-table-column>

    <el-table-column
      v-for="(col, colIndex) in props.columns"
      :key="colIndex"
      :label="col"
      min-width="120"
    >
      <template #default="{ row, $index }">
        <el-input
          :model-value="getDisplayValue($index, colIndex, row[colIndex])"
          @input="handleInput($index, colIndex, $event as string)"
          @blur="handleBlur($index, colIndex, ($event.target as HTMLInputElement).value)"
          size="small"
        />
      </template>
    </el-table-column>
  </el-table>
</template>

<style scoped>
.row-index {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}

.row-index .el-button {
  opacity: 0;
  transition: opacity 0.2s;
}

:deep(.el-table__row:hover) .row-index .el-button {
  opacity: 1;
}

:deep(.el-table .el-input__wrapper) {
  box-shadow: none;
}

:deep(.el-table .el-input__wrapper:hover) {
  box-shadow: 0 0 0 1px #409eff inset;
}
</style>
