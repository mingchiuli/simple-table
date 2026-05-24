<script setup lang="ts">
import {computed, h, onMounted, onUnmounted, ref, watch} from 'vue';
import type {CellValue, ColumnConfig, MergeRange, SortState} from '@/types';
import { EditableCell, RowNumberCell, ColumnHeaderCell } from '@/components/cell';
import { usePlatform } from '@/composables/usePlatform';

const { isTouchDevice } = usePlatform();

const props = defineProps<{
  data: CellValue[][];
  columns: string[];
  merges?: MergeRange[];
  selectedCell?: { row: number; col: number } | null;
  autoScroll?: boolean;
  sortState?: SortState | null;
  columnWidths?: Record<number, number>;
}>();

const emit = defineEmits<{
  (e: 'cell-change', rowIndex: number, colIndex: number, value: string): void;
  (e: 'delete-row', index: number): void;
  (e: 'delete-column', index: number): void;
  (e: 'select-cell', rowIndex: number, colIndex: number): void;
  (e: 'cell-editing', rowIndex: number, colIndex: number, value: string): void;
  (e: 'sort-column', colIndex: number, ascending: boolean): void;
  (e: 'column-resize', colIndex: number, width: number): void;
}>();

// 本地编辑状态
const editingValue = ref<Record<string, string>>({});
const editingCell = ref<string | null>(null);
const isManualClick = ref(false); // 是否手动点击触发的编辑

// 列宽状态
const columnWidths = ref<Record<number, number>>({});
const resizingColumn = ref<number | null>(null);
const startX = ref(0);
const startWidth = ref(0);
const resizeLineX = ref(0); // 实时拖动线位置（相对于容器）

// 初始化列宽
function initColumnWidths() {
  if (props.columnWidths) {
    columnWidths.value = { ...props.columnWidths };
  } else {
    columnWidths.value = {};
  }
}

initColumnWidths();

// 监听 columnWidths 变化
watch(() => props.columnWidths, initColumnWidths, { deep: true });

// 获取列宽
function getColumnWidth(colIndex: number): number {
  return columnWidths.value[colIndex] || 120;
}

// 获取 clientX（兼容鼠标和触摸事件）
function getClientX(event: MouseEvent | TouchEvent): number {
  if ('clientX' in event) {
    return event.clientX;
  }
  // TouchEvent - 使用第一个触摸点
  if (event.touches && event.touches.length > 0) {
    return event.touches[0].clientX;
  }
  // touchend 事件使用 changedTouches
  if (event.changedTouches && event.changedTouches.length > 0) {
    return event.changedTouches[0].clientX;
  }
  return 0;
}

// 开始调整列宽
function startResize(event: MouseEvent | TouchEvent, colIndex: number) {
  event.preventDefault();
  resizingColumn.value = colIndex;
  startX.value = getClientX(event);
  startWidth.value = getColumnWidth(colIndex);

  // 初始化拖动线位置
  if (containerRef.value) {
    const containerRect = containerRef.value.getBoundingClientRect();
    resizeLineX.value = startX.value - containerRect.left;
  }

  // 鼠标事件（始终添加）
  document.addEventListener('mousemove', onResize);
  document.addEventListener('mouseup', stopResize);

  // 触摸事件（仅在触摸设备上动态添加）
  if (isTouchDevice.value) {
    document.addEventListener('touchmove', onResize, { passive: false });
    document.addEventListener('touchend', stopResize);
  }
}

// 调整列宽中
function onResize(event: MouseEvent | TouchEvent) {
  if (resizingColumn.value === null) return;

  if (event.type === 'touchmove') {
    event.preventDefault();
  }

  const clientX = getClientX(event);

  // 实时更新拖动线位置（相对于容器）
  if (containerRef.value) {
    const containerRect = containerRef.value.getBoundingClientRect();
    resizeLineX.value = clientX - containerRect.left;
  }

  const delta = clientX - startX.value;
  columnWidths.value[resizingColumn.value] = Math.max(40, startWidth.value + delta);
}

// 结束调整列宽
function stopResize() {
  if (resizingColumn.value !== null) {
    emit('column-resize', resizingColumn.value, columnWidths.value[resizingColumn.value]);
  }
  resizingColumn.value = null;
  resizeLineX.value = 0;

  // 移除鼠标事件
  document.removeEventListener('mousemove', onResize);
  document.removeEventListener('mouseup', stopResize);

  // 移除触摸事件（仅在触摸设备上）
  if (isTouchDevice.value) {
    document.removeEventListener('touchmove', onResize);
    document.removeEventListener('touchend', stopResize);
  }
}

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
    const scrollTop = newCell.row * ROW_HEIGHT - tableSize.value.height / 2 + ROW_HEIGHT / 2;
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

// 获取单元格所在的合并区域信息
function getMergeInfo(rowIndex: number, colIndex: number): MergeRange | null {
  if (!props.merges) return null;

  for (const merge of props.merges) {
    if (
      rowIndex >= merge.startRow &&
      rowIndex <= merge.endRow &&
      colIndex >= merge.startCol &&
      colIndex <= merge.endCol
    ) {
      return merge;
    }
  }
  return null;
}

// el-table-v2 的 span-method
function spanMethod({ rowIndex, columnIndex }: { rowIndex: number; columnIndex: number }): { rowspan: number; colspan: number } | false {
  // columnIndex 0 是行号列，不参与合并
  if (columnIndex === 0) return { rowspan: 1, colspan: 1 };

  const dataColIndex = columnIndex - 1; // 数据列索引（从0开始）
  const merge = getMergeInfo(rowIndex, dataColIndex);

  // 没有合并区域，正常显示
  if (!merge) {
    return { rowspan: 1, colspan: 1 };
  }

  // 是合并区域的起始单元格，返回合并范围
  if (merge.startRow === rowIndex && merge.startCol === dataColIndex) {
    return {
      rowspan: merge.endRow - merge.startRow + 1,
      colspan: merge.endCol - merge.startCol + 1
    };
  }

  // 非起始单元格，隐藏
  return false;
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
  // 检查是否点击在合并区域内，如果是则跳转到起始单元格
  const merge = getMergeInfo(rowIndex, colIndex);
  if (merge) {
    rowIndex = merge.startRow;
    colIndex = merge.startCol;
  }

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
  const cols: ColumnConfig[] = [
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
      width: getColumnWidth(colIndex),
      headerCellRenderer: () => h(ColumnHeaderCell, {
        columnIndex: colIndex,
        title: col,
        width: getColumnWidth(colIndex),
        sortState: props.sortState,
        onDelete: handleDeleteColumn,
        onSort: (ascending: boolean) => emit('sort-column', colIndex, ascending),
        onResizeStart: startResize
      })
    });
  });

  return cols;
});

// 行高固定
const ROW_HEIGHT = 60;
</script>

<template>
  <div ref="containerRef" class="table-container">
    <el-table-v2
      ref="tableRef"
      :columns="columns"
      :data="props.data"
      :row-height="ROW_HEIGHT"
      :width="tableSize.width"
      :height="tableSize.height"
      :span-method="spanMethod"
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
    <!-- 拖动指示线 -->
    <div
      v-if="resizingColumn !== null"
      class="resize-line"
      :style="{ left: resizeLineX + 'px' }"
    />
  </div>
</template>

<style scoped>
.table-container {
  width: 100%;
  height: 100%;
  position: relative;
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

.resize-line {
  position: absolute;
  top: 0;
  bottom: 0;
  width: 2px;
  background-color: var(--el-color-primary);
  z-index: 100;
  pointer-events: none;
}
</style>
