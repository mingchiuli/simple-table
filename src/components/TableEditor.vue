<script setup lang="ts">
import type { CellValue, MergeRange } from '@/types';
import { CellView, ColumnHeaderCell, EditableCell, RowNumberCell } from '@/components/cell';
import { usePlatform } from '@/composables/usePlatform';
import { cellToDisplayString, cellToEditorString } from '@/composables/usePendingCellSave';

const { isTouchDevice } = usePlatform();

const DEFAULT_ROW_HEIGHT = 72;
const DEFAULT_COLUMN_WIDTH = 120;
const HEADER_HEIGHT = 50;
const ROW_HEADER_WIDTH = 60;
const MIN_COLUMN_WIDTH = 56;
const MIN_ROW_HEIGHT = 36;
const OVERSCAN_PX = 240;

type RowItem = {
  index: number;
  top: number;
  height: number;
};

type ColumnItem = {
  index: number;
  title: string;
  left: number;
  width: number;
};

type CellItem = {
  key: string;
  rowIndex: number;
  colIndex: number;
  top: number;
  left: number;
  width: number;
  height: number;
  value: CellValue | undefined;
};

type MergeOverlayCell = CellItem & {
  draftValue?: string;
  selected: boolean;
};

const props = defineProps<{
  data: CellValue[][];
  columns: string[];
  sheetIndex: number;
  draftCellValues?: Map<string, string>;
  merges?: MergeRange[];
  selectedCell?: { row: number; col: number } | null;
  autoScroll?: boolean;
  columnWidths?: Record<number, number>;
  rowHeights?: Record<number, number>;
}>();

const emit = defineEmits<{
  (e: 'cell-change', rowIndex: number, colIndex: number, value: string): void;
  (e: 'delete-row', index: number): void;
  (e: 'delete-column', index: number): void;
  (e: 'select-cell', rowIndex: number, colIndex: number): void;
  (e: 'cell-editing', rowIndex: number, colIndex: number, value: string): void;
  (e: 'cell-edit-cancel', rowIndex: number, colIndex: number): void;
  (e: 'column-resize', colIndex: number, width: number): void;
  (e: 'row-resize', rowIndex: number, height: number): void;
}>();

const containerRef = ref<HTMLElement | null>(null);
const scrollViewportRef = ref<HTMLElement | null>(null);
const tableSize = ref({ width: 800, height: 600 });

const editingValue = ref<Record<string, string>>({});
const editingCell = ref<string | null>(null);
const isManualClick = ref(false);

const columnWidths = ref<Record<number, number>>({});
const rowHeights = ref<Record<number, number>>({});
const resizingColumn = ref<number | null>(null);
const resizingRow = ref<number | null>(null);
const startX = ref(0);
const startY = ref(0);
const startWidth = ref(0);
const startHeight = ref(0);
const resizeLineX = ref(0);
const resizeLineY = ref(0);
const scrollLeft = ref(0);
const scrollTop = ref(0);

function initColumnWidths() {
  const nextWidths = props.columnWidths ? { ...props.columnWidths } : {};
  if (areNumberRecordsEqual(columnWidths.value, nextWidths)) return;
  columnWidths.value = nextWidths;
}

function initRowHeights() {
  const nextHeights = props.rowHeights ? { ...props.rowHeights } : {};
  if (areNumberRecordsEqual(rowHeights.value, nextHeights)) return;
  rowHeights.value = nextHeights;
}

initColumnWidths();
initRowHeights();

watch(() => props.columnWidths, initColumnWidths, { deep: true });
watch(() => props.rowHeights, initRowHeights, { deep: true });

const viewportWidth = computed(() => Math.max(0, tableSize.value.width - ROW_HEADER_WIDTH));
const viewportHeight = computed(() => Math.max(0, tableSize.value.height - HEADER_HEIGHT));

const columnOffsets = computed(() => {
  const offsets = [0];
  for (let colIndex = 0; colIndex < props.columns.length; colIndex += 1) {
    offsets.push(offsets[colIndex] + getColumnWidth(colIndex));
  }
  return offsets;
});

const rowOffsets = computed(() => {
  const offsets = [0];
  for (let rowIndex = 0; rowIndex < props.data.length; rowIndex += 1) {
    offsets.push(offsets[rowIndex] + getRowHeight(rowIndex));
  }
  return offsets;
});

const totalColumnsWidth = computed(() => columnOffsets.value.at(-1) ?? 0);
const totalRowsHeight = computed(() => rowOffsets.value.at(-1) ?? 0);

const visibleRows = computed<RowItem[]>(() => {
  return collectVisibleItems(rowOffsets.value, props.data.length, scrollTop.value, viewportHeight.value);
});

const visibleColumns = computed<ColumnItem[]>(() => {
  return collectVisibleItems(columnOffsets.value, props.columns.length, scrollLeft.value, viewportWidth.value)
    .map((item) => ({
      index: item.index,
      title: props.columns[item.index] ?? '',
      left: item.top,
      width: item.height,
    }));
});

const visibleCellItems = computed<CellItem[]>(() => {
  const cells: CellItem[] = [];
  for (const row of visibleRows.value) {
    for (const column of visibleColumns.value) {
      if (isMergedCell(row.index, column.index)) continue;
      cells.push({
        key: `${row.index}-${column.index}`,
        rowIndex: row.index,
        colIndex: column.index,
        top: row.top,
        left: column.left,
        width: column.width,
        height: row.height,
        value: props.data[row.index]?.[column.index],
      });
    }
  }
  return cells;
});

const visibleMergeCells = computed<MergeOverlayCell[]>(() => {
  const merges = props.merges ?? [];
  const leftLimit = scrollLeft.value - OVERSCAN_PX;
  const rightLimit = scrollLeft.value + viewportWidth.value + OVERSCAN_PX;
  const topLimit = scrollTop.value - OVERSCAN_PX;
  const bottomLimit = scrollTop.value + viewportHeight.value + OVERSCAN_PX;

  return merges.flatMap((merge) => {
    const left = getDataColumnOffset(merge.startCol);
    const top = getRowOffset(merge.startRow);
    const width = getColumnSpanWidth(merge.startCol, merge.endCol);
    const height = getRowSpanHeight(merge.startRow, merge.endRow);

    if (
      width <= 0
      || height <= 0
      || left + width < leftLimit
      || left > rightLimit
      || top + height < topLimit
      || top > bottomLimit
    ) {
      return [];
    }

    return [{
      key: `${merge.startRow}-${merge.startCol}-${merge.endRow}-${merge.endCol}`,
      rowIndex: merge.startRow,
      colIndex: merge.startCol,
      top,
      left,
      width,
      height,
      value: props.data[merge.startRow]?.[merge.startCol],
      draftValue: getDraftValue(merge.startRow, merge.startCol),
      selected: isSelectedCell(merge.startRow, merge.startCol),
    }];
  });
});

const visibleColumnResizeHandles = computed(() => {
  const handles: Array<{ colIndex: number; left: number }> = [];
  let boundary = ROW_HEADER_WIDTH - scrollLeft.value;
  for (let colIndex = 0; colIndex < props.columns.length; colIndex += 1) {
    boundary += getColumnWidth(colIndex);
    if (boundary >= ROW_HEADER_WIDTH && boundary <= tableSize.value.width) {
      handles.push({ colIndex, left: boundary });
    }
    if (boundary > tableSize.value.width) break;
  }
  return handles;
});

function collectVisibleItems(
  offsets: number[],
  count: number,
  scrollStart: number,
  viewportSize: number
): RowItem[] {
  if (count <= 0) return [];

  const start = Math.max(0, scrollStart - OVERSCAN_PX);
  const end = scrollStart + viewportSize + OVERSCAN_PX;
  const firstIndex = findFirstVisibleIndex(offsets, start);
  const items: RowItem[] = [];

  for (let index = firstIndex; index < count; index += 1) {
    const top = offsets[index] ?? 0;
    const nextTop = offsets[index + 1] ?? top;
    if (top > end) break;
    items.push({ index, top, height: nextTop - top });
  }

  return items;
}

function findFirstVisibleIndex(offsets: number[], start: number): number {
  let low = 0;
  let high = Math.max(0, offsets.length - 2);
  let result = 0;

  while (low <= high) {
    const mid = Math.floor((low + high) / 2);
    if ((offsets[mid + 1] ?? 0) < start) {
      low = mid + 1;
    } else {
      result = mid;
      high = mid - 1;
    }
  }

  return result;
}

function getColumnWidth(colIndex: number): number {
  return columnWidths.value[colIndex] || DEFAULT_COLUMN_WIDTH;
}

function getRowHeight(rowIndex: number): number {
  return rowHeights.value[rowIndex] || DEFAULT_ROW_HEIGHT;
}

function getRowOffset(rowIndex: number): number {
  const clamped = Math.max(0, Math.min(rowIndex, rowOffsets.value.length - 1));
  return rowOffsets.value[clamped] ?? totalRowsHeight.value;
}

function getColumnOffset(colIndex: number): number {
  return ROW_HEADER_WIDTH + getDataColumnOffset(colIndex);
}

function getDataColumnOffset(colIndex: number): number {
  const clamped = Math.max(0, Math.min(colIndex, columnOffsets.value.length - 1));
  return columnOffsets.value[clamped] ?? totalColumnsWidth.value;
}

function getColumnSpanWidth(startCol: number, endCol: number): number {
  const start = getDataColumnOffset(startCol);
  const end = getDataColumnOffset(endCol + 1);
  return Math.max(0, end - start);
}

function getRowSpanHeight(startRow: number, endRow: number): number {
  const start = getRowOffset(startRow);
  const end = getRowOffset(endRow + 1);
  return Math.max(0, end - start);
}

function areNumberRecordsEqual(
  current: Record<number, number>,
  next: Record<number, number>
): boolean {
  const currentKeys = Object.keys(current);
  const nextKeys = Object.keys(next);
  if (currentKeys.length !== nextKeys.length) return false;
  return currentKeys.every((key) => current[Number(key)] === next[Number(key)]);
}

function handleViewportScroll() {
  const viewport = scrollViewportRef.value;
  if (!viewport) return;
  scrollLeft.value = viewport.scrollLeft;
  scrollTop.value = viewport.scrollTop;
}

function getClientX(event: MouseEvent | TouchEvent): number {
  if ('clientX' in event) return event.clientX;
  if (event.touches && event.touches.length > 0) return event.touches[0].clientX;
  if (event.changedTouches && event.changedTouches.length > 0) return event.changedTouches[0].clientX;
  return 0;
}

function getClientY(event: MouseEvent | TouchEvent): number {
  if ('clientY' in event) return event.clientY;
  if (event.touches && event.touches.length > 0) return event.touches[0].clientY;
  if (event.changedTouches && event.changedTouches.length > 0) return event.changedTouches[0].clientY;
  return 0;
}

function startResize(event: MouseEvent | TouchEvent, colIndex: number, boundaryX: number) {
  event.preventDefault();
  resizingColumn.value = colIndex;
  startX.value = getClientX(event);
  startWidth.value = getColumnWidth(colIndex);
  resizeLineX.value = boundaryX;

  document.addEventListener('mousemove', onResize);
  document.addEventListener('mouseup', stopResize);

  if (isTouchDevice.value) {
    document.addEventListener('touchmove', onResize, { passive: false });
    document.addEventListener('touchend', stopResize);
  }
}

function startRowResize(event: MouseEvent | TouchEvent, rowIndex: number) {
  event.preventDefault();
  resizingRow.value = rowIndex;
  startY.value = getClientY(event);
  startHeight.value = getRowHeight(rowIndex);
  resizeLineY.value = HEADER_HEIGHT + getRowOffset(rowIndex) + startHeight.value - scrollTop.value;

  document.addEventListener('mousemove', onResize);
  document.addEventListener('mouseup', stopResize);

  if (isTouchDevice.value) {
    document.addEventListener('touchmove', onResize, { passive: false });
    document.addEventListener('touchend', stopResize);
  }
}

function onResize(event: MouseEvent | TouchEvent) {
  if (resizingColumn.value === null && resizingRow.value === null) return;

  if (event.type === 'touchmove') {
    event.preventDefault();
  }

  if (resizingColumn.value !== null) {
    const delta = getClientX(event) - startX.value;
    const nextWidth = Math.max(MIN_COLUMN_WIDTH, startWidth.value + delta);
    columnWidths.value[resizingColumn.value] = nextWidth;
    resizeLineX.value = getColumnOffset(resizingColumn.value) + nextWidth - scrollLeft.value;
  }

  if (resizingRow.value !== null) {
    const delta = getClientY(event) - startY.value;
    const nextHeight = Math.max(MIN_ROW_HEIGHT, startHeight.value + delta);
    rowHeights.value[resizingRow.value] = nextHeight;
    resizeLineY.value = HEADER_HEIGHT + getRowOffset(resizingRow.value) + nextHeight - scrollTop.value;
  }
}

function stopResize() {
  if (resizingColumn.value !== null) {
    emit('column-resize', resizingColumn.value, columnWidths.value[resizingColumn.value]);
  }

  if (resizingRow.value !== null) {
    emit('row-resize', resizingRow.value, rowHeights.value[resizingRow.value]);
  }

  resizingColumn.value = null;
  resizingRow.value = null;
  resizeLineX.value = 0;
  resizeLineY.value = 0;

  document.removeEventListener('mousemove', onResize);
  document.removeEventListener('mouseup', stopResize);

  if (isTouchDevice.value) {
    document.removeEventListener('touchmove', onResize);
    document.removeEventListener('touchend', stopResize);
  }
}

watch(() => props.data, () => {
  if (!props.selectedCell) return;

  const key = getKey(props.selectedCell.row, props.selectedCell.col);
  if (editingValue.value[key] === undefined) return;

  editingValue.value[key] = getDraftValue(props.selectedCell.row, props.selectedCell.col)
    ?? getCellValue(props.data[props.selectedCell.row]?.[props.selectedCell.col])
    ?? '';
}, { deep: true });

watch(() => props.selectedCell, (newCell) => {
  if (!newCell) {
    editingCell.value = null;
    editingValue.value = {};
    return;
  }

  const newKey = getKey(newCell.row, newCell.col);
  if (editingCell.value === newKey) return;

  editingCell.value = null;
  editingValue.value = {};
  isManualClick.value = false;

  if (props.autoScroll && scrollViewportRef.value) {
    const targetTop = getRowOffset(newCell.row) - viewportHeight.value / 2 + getRowHeight(newCell.row) / 2;
    const targetLeft = getDataColumnOffset(newCell.col) - viewportWidth.value / 2 + getColumnWidth(newCell.col) / 2;
    scrollViewportRef.value.scrollTo({
      top: Math.max(0, targetTop),
      left: Math.max(0, targetLeft),
    });
  }
}, { deep: true });

let resizeObserver: ResizeObserver | null = null;

onMounted(() => {
  if (!containerRef.value) return;

  const updateSize = () => {
    tableSize.value = {
      width: containerRef.value!.clientWidth,
      height: containerRef.value!.clientHeight,
    };
  };

  updateSize();
  resizeObserver = new ResizeObserver(updateSize);
  resizeObserver.observe(containerRef.value);
});

onUnmounted(() => {
  resizeObserver?.disconnect();
});

function getCellValue(cell: CellValue | undefined): string {
  return cellToEditorString(cell);
}

function getKey(rowIndex: number, colIndex: number): string {
  return `${rowIndex}-${colIndex}`;
}

function getDraftKey(rowIndex: number, colIndex: number): string {
  return `${props.sheetIndex},${rowIndex},${colIndex}`;
}

function getDraftValue(rowIndex: number, colIndex: number): string | undefined {
  return props.draftCellValues?.get(getDraftKey(rowIndex, colIndex));
}

function getMergeInfo(rowIndex: number, colIndex: number): MergeRange | null {
  if (!props.merges) return null;

  for (const merge of props.merges) {
    if (
      rowIndex >= merge.startRow
      && rowIndex <= merge.endRow
      && colIndex >= merge.startCol
      && colIndex <= merge.endCol
    ) {
      return merge;
    }
  }

  return null;
}

function isMergedCell(rowIndex: number, colIndex: number): boolean {
  return getMergeInfo(rowIndex, colIndex) !== null;
}

function handleInput(rowIndex: number, colIndex: number, value: string) {
  editingValue.value[getKey(rowIndex, colIndex)] = value;
  emit('cell-editing', rowIndex, colIndex, value);
}

function handleBlur(rowIndex: number, colIndex: number, value: string) {
  const key = getKey(rowIndex, colIndex);
  const originalValue = getCellValue(props.data[rowIndex]?.[colIndex]);

  if (value !== originalValue || getDraftValue(rowIndex, colIndex) !== undefined) {
    emit('cell-change', rowIndex, colIndex, value);
  }

  delete editingValue.value[key];
  editingCell.value = null;
}

function handleCancelEdit(rowIndex: number, colIndex: number) {
  const key = getKey(rowIndex, colIndex);
  delete editingValue.value[key];
  editingCell.value = null;
  emit('cell-edit-cancel', rowIndex, colIndex);
}

function handleDeleteRow(index: number) {
  emit('delete-row', index);
}

function handleDeleteColumn(index: number) {
  emit('delete-column', index);
}

function getDisplayValue(rowIndex: number, colIndex: number, cellValue: CellValue | undefined): string {
  const key = getKey(rowIndex, colIndex);
  if (editingValue.value[key] !== undefined) return editingValue.value[key];

  const draftValue = getDraftValue(rowIndex, colIndex);
  if (draftValue !== undefined) return draftValue;

  return cellToDisplayString(cellValue);
}

function isEditing(rowIndex: number, colIndex: number): boolean {
  return editingCell.value === getKey(rowIndex, colIndex);
}

function isSelectedCell(rowIndex: number, colIndex: number): boolean {
  return props.selectedCell?.row === rowIndex && props.selectedCell?.col === colIndex;
}

function handleCellClick(rowIndex: number, colIndex: number) {
  const merge = getMergeInfo(rowIndex, colIndex);
  if (merge) {
    rowIndex = merge.startRow;
    colIndex = merge.startCol;
  }

  emit('select-cell', rowIndex, colIndex);
}

function handleCellDoubleClick(rowIndex: number, colIndex: number) {
  const merge = getMergeInfo(rowIndex, colIndex);
  if (merge) {
    rowIndex = merge.startRow;
    colIndex = merge.startCol;
  }

  emit('select-cell', rowIndex, colIndex);
  const key = getKey(rowIndex, colIndex);
  editingCell.value = key;
  editingValue.value = {};
  editingValue.value[key] = getDraftValue(rowIndex, colIndex)
    ?? getCellValue(props.data[rowIndex]?.[colIndex])
    ?? '';
  isManualClick.value = true;
}
</script>

<template>
  <div
    ref="containerRef"
    class="table-container"
    :style="{
      '--row-header-width': `${ROW_HEADER_WIDTH}px`,
      '--table-header-height': `${HEADER_HEIGHT}px`,
    }"
  >
    <div class="corner-cell">#</div>

    <div class="column-header-viewport">
      <div class="column-header-strip" :style="{ width: `${totalColumnsWidth}px` }">
        <div
          v-for="column in visibleColumns"
          :key="column.index"
          class="column-header-cell"
          :style="{
            left: `${column.left - scrollLeft}px`,
            width: `${column.width}px`,
          }"
        >
          <ColumnHeaderCell
            :column-index="column.index"
            :title="column.title"
            @delete="handleDeleteColumn"
          />
        </div>
      </div>
    </div>

    <div class="row-header-viewport">
      <div class="row-header-strip" :style="{ height: `${totalRowsHeight}px` }">
        <div
          v-for="row in visibleRows"
          :key="row.index"
          class="row-header-cell"
          :style="{
            top: `${row.top - scrollTop}px`,
            height: `${row.height}px`,
          }"
        >
          <RowNumberCell
            :row-index="row.index"
            @delete="handleDeleteRow"
            @resize-start="startRowResize"
          />
        </div>
      </div>
    </div>

    <div ref="scrollViewportRef" class="data-viewport" @scroll="handleViewportScroll">
      <div
        class="data-scroll-content"
        :style="{
          width: `${totalColumnsWidth}px`,
          height: `${totalRowsHeight}px`,
        }"
      >
        <div
          v-for="cell in visibleCellItems"
          :key="cell.key"
          class="data-cell"
          :class="{ 'is-selected': isSelectedCell(cell.rowIndex, cell.colIndex) }"
          :style="{
            left: `${cell.left}px`,
            top: `${cell.top}px`,
            width: `${cell.width}px`,
            height: `${cell.height}px`,
          }"
          @click="handleCellClick(cell.rowIndex, cell.colIndex)"
          @dblclick="handleCellDoubleClick(cell.rowIndex, cell.colIndex)"
        >
          <CellView
            v-if="!isEditing(cell.rowIndex, cell.colIndex)"
            :value="cell.value"
            :draft-value="getDraftValue(cell.rowIndex, cell.colIndex)"
            :selected="false"
            :row-height="cell.height"
          />
          <EditableCell
            v-else
            :auto-focus="isManualClick"
            :min-height="cell.height"
            :model-value="editingValue[getKey(cell.rowIndex, cell.colIndex)] ?? getDisplayValue(cell.rowIndex, cell.colIndex, cell.value)"
            @update:model-value="(val: string) => handleInput(cell.rowIndex, cell.colIndex, val)"
            @commit="handleBlur(cell.rowIndex, cell.colIndex, editingValue[getKey(cell.rowIndex, cell.colIndex)] ?? getDisplayValue(cell.rowIndex, cell.colIndex, cell.value))"
            @cancel="handleCancelEdit(cell.rowIndex, cell.colIndex)"
          />
        </div>

        <div
          v-for="mergeCell in visibleMergeCells"
          :key="mergeCell.key"
          class="merge-cell"
          :class="{ 'is-selected': mergeCell.selected, 'is-editing': isEditing(mergeCell.rowIndex, mergeCell.colIndex) }"
          :style="{
            left: `${mergeCell.left}px`,
            top: `${mergeCell.top}px`,
            width: `${mergeCell.width}px`,
            height: `${mergeCell.height}px`,
          }"
          @click="handleCellClick(mergeCell.rowIndex, mergeCell.colIndex)"
          @dblclick="handleCellDoubleClick(mergeCell.rowIndex, mergeCell.colIndex)"
        >
          <CellView
            v-if="!isEditing(mergeCell.rowIndex, mergeCell.colIndex)"
            :value="mergeCell.value"
            :draft-value="mergeCell.draftValue"
            :selected="false"
            :row-height="mergeCell.height"
          />
          <EditableCell
            v-else
            :auto-focus="isManualClick"
            :min-height="mergeCell.height"
            :model-value="editingValue[getKey(mergeCell.rowIndex, mergeCell.colIndex)] ?? getDisplayValue(mergeCell.rowIndex, mergeCell.colIndex, mergeCell.value)"
            @update:model-value="(val: string) => handleInput(mergeCell.rowIndex, mergeCell.colIndex, val)"
            @commit="handleBlur(mergeCell.rowIndex, mergeCell.colIndex, editingValue[getKey(mergeCell.rowIndex, mergeCell.colIndex)] ?? getDisplayValue(mergeCell.rowIndex, mergeCell.colIndex, mergeCell.value))"
            @cancel="handleCancelEdit(mergeCell.rowIndex, mergeCell.colIndex)"
          />
        </div>
      </div>
    </div>

    <div class="resize-overlay" aria-hidden="true">
      <div
        v-for="handle in visibleColumnResizeHandles"
        :key="handle.colIndex"
        class="column-resize-handle"
        :class="{ 'is-active': resizingColumn === handle.colIndex }"
        :style="{ left: `${handle.left}px` }"
        @mousedown.stop="startResize($event, handle.colIndex, handle.left)"
        @touchstart.stop="(event: TouchEvent) => isTouchDevice && startResize(event, handle.colIndex, handle.left)"
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
  </div>
</template>

<style scoped>
.table-container {
  --grid-border-color: var(--el-border-color-lighter);
  width: 100%;
  height: 100%;
  position: relative;
  min-width: 0;
  min-height: 0;
  overflow: hidden;
  background: var(--el-bg-color);
  color: var(--el-text-color-primary);
  font-size: 14px;
  touch-action: pan-x pan-y;
}

.corner-cell,
.column-header-cell,
.row-header-cell {
  background: var(--el-fill-color-blank);
}

.corner-cell {
  position: absolute;
  left: 0;
  top: 0;
  width: var(--row-header-width, 60px);
  height: var(--table-header-height, 50px);
  z-index: 70;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--el-text-color-secondary);
  font-weight: 700;
  box-shadow:
    inset -1px 0 0 var(--grid-border-color),
    inset 0 -1px 0 var(--grid-border-color);
}

.column-header-viewport {
  position: absolute;
  left: var(--row-header-width, 60px);
  right: 0;
  top: 0;
  height: var(--table-header-height, 50px);
  z-index: 60;
  overflow: hidden;
  background: var(--el-fill-color-blank);
}

.column-header-strip,
.row-header-strip,
.data-scroll-content {
  position: relative;
}

.column-header-cell {
  position: absolute;
  top: 0;
  height: var(--table-header-height, 50px);
  box-sizing: border-box;
  box-shadow:
    inset -1px 0 0 var(--grid-border-color),
    inset 0 -1px 0 var(--grid-border-color);
}

.row-header-viewport {
  position: absolute;
  left: 0;
  top: var(--table-header-height, 50px);
  bottom: 0;
  width: var(--row-header-width, 60px);
  z-index: 60;
  overflow: hidden;
  background: var(--el-fill-color-blank);
}

.row-header-cell {
  position: absolute;
  left: 0;
  width: var(--row-header-width, 60px);
  box-sizing: border-box;
  box-shadow:
    inset -1px 0 0 var(--grid-border-color),
    inset 0 -1px 0 var(--grid-border-color);
}

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
  min-width: 100%;
  min-height: 100%;
}

.data-cell,
.merge-cell {
  position: absolute;
  box-sizing: border-box;
  background: var(--el-bg-color);
  overflow: hidden;
}

.data-cell {
  z-index: 1;
  box-shadow:
    inset -1px 0 0 var(--grid-border-color),
    inset 0 -1px 0 var(--grid-border-color);
}

.merge-cell {
  z-index: 3;
  box-shadow: inset 0 0 0 1px var(--grid-border-color);
}

.data-cell.is-selected::after,
.merge-cell.is-selected::after {
  content: "";
  position: absolute;
  inset: -1px;
  border: 2px solid var(--el-color-primary);
  pointer-events: none;
  z-index: 5;
}

.merge-cell.is-editing {
  z-index: 4;
}

.resize-overlay {
  position: absolute;
  inset: 0;
  z-index: 90;
  pointer-events: none;
}

.column-resize-handle {
  position: absolute;
  top: 0;
  bottom: 0;
  width: 10px;
  transform: translateX(-5px);
  cursor: col-resize;
  pointer-events: auto;
}

.column-resize-handle::after {
  content: "";
  position: absolute;
  top: 0;
  bottom: 0;
  left: 4px;
  width: 2px;
  background: transparent;
}

.column-resize-handle:hover::after,
.column-resize-handle.is-active::after {
  background: var(--el-color-primary);
}

.resize-line {
  position: absolute;
  top: 0;
  bottom: 0;
  width: 2px;
  background-color: var(--el-color-primary);
  z-index: 100;
  pointer-events: none;
  transform: translateX(-1px);
}

.resize-line.horizontal {
  left: 0;
  right: 0;
  bottom: auto;
  width: auto;
  height: 2px;
  transform: translateY(-1px);
}

@media (pointer: coarse) and (hover: none) {
  .table-container {
    font-size: 16px;
  }

  .data-viewport {
    -webkit-overflow-scrolling: touch;
  }

  .column-resize-handle {
    width: 18px;
    transform: translateX(-9px);
  }

  .column-resize-handle::after {
    left: 8px;
  }
}
</style>
