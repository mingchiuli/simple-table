<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, watch, h } from 'vue';
import type { CellValue } from '@/types';
import EditableCell from './EditableCell.vue';
import RowNumberCell from './RowNumberCell.vue';
import ColumnHeaderCell from './ColumnHeaderCell.vue';

const props = defineProps<{
  data: CellValue[][];
  columns: string[];
  selectedCell?: { row: number; col: number } | null;
  autoScroll?: boolean;
}>();

const emit = defineEmits<{
  (e: 'cell-change', rowIndex: number, colIndex: number, value: string): void;
  (e: 'delete-row', index: number): void;
  (e: 'delete-column', index: number): void;
  (e: 'select-cell', rowIndex: number, colIndex: number): void;
  (e: 'cell-editing', rowIndex: number, colIndex: number, value: string): void;
}>();

// 本地编辑状态
const editingValue = ref<Record<string, string>>({});
const editingCell = ref<string | null>(null);
const isManualClick = ref(false); // 是否手动点击触发的编辑

// 监听 data 变化，更新当前编辑单元格的值（实现实时同步）
watch(() => props.data, () => {
  if (props.selectedCell) {
    const key = getKey(props.selectedCell.row, props.selectedCell.col);
    if (editingValue.value[key] !== undefined) {
      // 外部数据变化时，同步更新 editingValue
      editingValue.value[key] = getCellValue(props.data[props.selectedCell.row]?.[props.selectedCell.col]) || '';
    }
  }
}, { deep: true });

// 容器尺寸
const containerRef = ref<HTMLElement | null>(null);
const tableRef = ref<any>(null);
const tableSize = ref({ width: 800, height: 600 });

// 监听选中单元格变化，进入编辑模式
watch(() => props.selectedCell, async (newCell) => {
  // Clear edit state when switching sheets (selectedCell becomes null)
  if (!newCell) {
    editingCell.value = null;
    editingValue.value = {};
    return;
  }

  // 如果是本地点击触发的（selectedCell 变化但 editingCell 已同步），不重复处理
  const newKey = getKey(newCell.row, newCell.col);
  if (editingCell.value === newKey) {
    return;
  }

  // 外部触发（如搜索侧边栏），不自动聚焦
  isManualClick.value = false;

  // Enter edit mode when a cell is selected
  const key = getKey(newCell.row, newCell.col);
  editingCell.value = key;
  editingValue.value = {};
  editingValue.value[key] = getCellValue(props.data[newCell.row]?.[newCell.col]) || '';

  // Only scroll when autoScroll is true (e.g., from search results)
  if (props.autoScroll && tableRef.value) {
    const rowHeight = 40;
    const scrollTop = newCell.row * rowHeight - tableSize.value.height / 2 + rowHeight / 2;
    const rowNumberWidth = 60;
    const colWidth = 120;
    const scrollLeft = rowNumberWidth + newCell.col * colWidth - tableSize.value.width / 2 + colWidth / 2;

    if (typeof tableRef.value.scrollToTop === 'function') {
      tableRef.value.scrollToTop(Math.max(0, scrollTop));
    }
    if (typeof tableRef.value.scrollToLeft === 'function') {
      tableRef.value.scrollToLeft(Math.max(0, scrollLeft));
    }
  }
}, { deep: true });

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
  // 实时同步到上方编辑栏
  emit('cell-editing', rowIndex, colIndex, value);
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

function handleDeleteColumn(index: number) {
  emit('delete-column', index);
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

function handleCellClick(rowIndex: number, colIndex: number) {
  // 单击选中单元格并显示编辑栏
  emit('select-cell', rowIndex, colIndex);

  // 双击进入编辑模式（通过检查是否已选中来判断）
  const key = getKey(rowIndex, colIndex);
  if (editingCell.value === key) {
    // 已经是编辑状态，保持
  } else {
    // 选中单元格，进入编辑模式
    editingCell.value = key;
    // 标记为手动点击，启用自动聚焦
    isManualClick.value = true;
  }
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
      headerCellRenderer: () => h(ColumnHeaderCell, {
        columnIndex: colIndex,
        title: col,
        onDelete: handleDeleteColumn
      })
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
      ref="tableRef"
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
            @click="handleCellClick(rowIndex, column.dataKey)"
          >
            {{ getDisplayValue(rowIndex, column.dataKey, rowData[column.dataKey]) }}
          </div>
          <EditableCell
            v-else
            :auto-focus="isManualClick"
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
