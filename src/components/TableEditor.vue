<script setup lang="ts">
import type { CellValue } from '@/types';

const props = defineProps<{
  data: CellValue[][];
  columns: string[];
}>();

const emit = defineEmits<{
  (e: 'cell-change', rowIndex: number, colIndex: number, value: string): void;
  (e: 'delete-row', index: number): void;
}>();

function getCellValue(cell: CellValue): string {
  if (cell === null || cell === undefined) return '';
  return String(cell);
}

function handleCellChange(rowIndex: number, colIndex: number, value: string) {
  emit('cell-change', rowIndex, colIndex, value);
}

function handleDeleteRow(index: number) {
  emit('delete-row', index);
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
          :model-value="getCellValue(row[colIndex])"
          @update:model-value="(val: string) => handleCellChange($index, colIndex, val)"
          size="small"
          borderless
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
