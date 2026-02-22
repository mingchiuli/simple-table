<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue';
import type { CellValue } from '@/types';
import EditableCell from './EditableCell.vue';
import RowNumberCell from './RowNumberCell.vue';

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
const editingCell = ref<string | null>(null);

// 容器尺寸
const containerRef = ref<HTMLElement | null>(null);
const tableSize = ref({ width: 800, height: 600 });

// 监听容器尺寸变化
let resizeObserver: ResizeObserver | null = null;

onMounted(() => {
  if (containerRef.value) {
    const updateSize = () => {
      tableSize.value = {
        width: containerRef.value!.clientWidth,
        height: containerRef.value!.clientHeight
      };
    };
    updateSize();
    resizeObserver = new ResizeObserver(updateSize);
    resizeObserver.observe(containerRef.value);
  }
});

onUnmounted(() => {
  if (resizeObserver) {
    resizeObserver.disconnect();
  }
});

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

  delete editingValue.value[key];
  editingCell.value = null;
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

function isEditing(rowIndex: number, colIndex: number): boolean {
  return editingCell.value === getKey(rowIndex, colIndex);
}

function startEditing(rowIndex: number, colIndex: number) {
  editingCell.value = getKey(rowIndex, colIndex);
}

// 列配置
const columns = computed(() => {
  const cols: any[] = [
    {
      key: 'row-number',
      title: '#',
      width: 60,
      fixed: 'left',
    }
  ];

  props.columns.forEach((col, colIndex) => {
    cols.push({
      key: `col-${colIndex}`,
      title: col,
      dataKey: colIndex,
      width: 120,
    });
  });

  return cols;
});

// 行高固定
const rowHeight = 40;
</script>

<template>
  <div ref="containerRef" class="table-container">
    <el-table-v2
      :columns="columns"
      :data="props.data"
      :row-height="rowHeight"
      :width="tableSize.width"
      :height="tableSize.height"
      fixed
    >
      <template #cell="{ column, rowData, rowIndex }">
        <!-- 行号列 -->
        <template v-if="column.key === 'row-number'">
          <RowNumberCell
            :row-index="rowIndex"
            @delete="handleDeleteRow"
          />
        </template>

        <!-- 数据列 -->
        <template v-else>
          <div
            v-if="!isEditing(rowIndex, column.dataKey)"
            class="cell-text"
            @click="startEditing(rowIndex, column.dataKey)"
          >
            {{ getDisplayValue(rowIndex, column.dataKey, rowData[column.dataKey]) }}
          </div>
          <EditableCell
            v-else
            :model-value="editingValue[getKey(rowIndex, column.dataKey)] ?? getDisplayValue(rowIndex, column.dataKey, rowData[column.dataKey])"
            @update:model-value="(val: string) => handleInput(rowIndex, column.dataKey, val)"
            @blur="handleBlur(rowIndex, column.dataKey, editingValue[getKey(rowIndex, column.dataKey)] ?? getDisplayValue(rowIndex, column.dataKey, rowData[column.dataKey]))"
          />
        </template>
      </template>
    </el-table-v2>
  </div>
</template>

<style scoped>
.table-container {
  width: 100%;
  height: 100%;
}

:deep(.el-table-v2) {
  font-size: 14px;
}

:deep(.el-table-v2__row-cell) {
  padding: 0 8px;
}

.cell-text {
  width: 100%;
  height: 100%;
  display: flex;
  align-items: center;
  cursor: text;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
