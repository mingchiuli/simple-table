<script setup lang="ts">
import { computed } from 'vue';
import { Sort, Close } from '@element-plus/icons-vue';
import { usePlatform } from '@/composables/usePlatform';
import type { SortState } from '@/types';

const { isTouchDevice } = usePlatform();

const props = defineProps<{
  columnIndex: number;
  title: string;
  width?: number;
  sortState?: SortState | null;
}>();

const emit = defineEmits<{
  (e: 'delete', index: number): void;
  (e: 'sort', ascending: boolean): void;
  (e: 'resize-start', event: MouseEvent | TouchEvent, colIndex: number): void;
}>();

function handleDelete(index: number) {
  emit('delete', index);
}

function handleSort() {
  const isCurrentColumn = props.sortState?.colIndex === props.columnIndex;
  const newAscending = isCurrentColumn && props.sortState ? !props.sortState.ascending : true;
  emit('sort', newAscending);
}

// 处理 resize 事件
function handleResizeStart(e: MouseEvent | TouchEvent) {
  emit('resize-start', e, props.columnIndex);
}

// 判断当前列是否正在排序
const isCurrentSorting = computed(() => props.sortState?.colIndex === props.columnIndex);
</script>

<template>
  <div class="column-header">
    <span class="title">{{ title }}</span>
    <div class="actions">
      <button
        class="sort-btn"
        :class="{ active: isCurrentSorting }"
        @click.stop="handleSort"
      >
        <el-icon :size="12"><Sort /></el-icon>
      </button>
      <button class="delete-btn" @click.stop="handleDelete(columnIndex)">
        <el-icon :size="12"><Close /></el-icon>
      </button>
    </div>
    <div
      class="resize-handle"
      @mousedown.stop="handleResizeStart"
      @touchstart.stop="(e: TouchEvent) => isTouchDevice && handleResizeStart(e)"
    />
  </div>
</template>

<style scoped>
.column-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  width: 100%;
  height: 100%;
  padding: 0 4px;
  position: relative;
}

.title {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.actions {
  display: flex;
  gap: 2px;
}

.sort-btn,
.delete-btn {
  opacity: 0;
  border: none;
  background: none;
  cursor: pointer;
  padding: 2px;
  border-radius: 4px;
  transition: opacity 0.2s, background-color 0.2s;
}

.sort-btn { color: #409eff; }
.sort-btn:hover { background-color: #ecf5ff; }
.sort-btn.active { opacity: 1; color: #409eff; font-weight: bold; }

.delete-btn { color: #f56c6c; }
.delete-btn:hover { background-color: #fef0f0; }

.column-header:hover .sort-btn,
.column-header:hover .delete-btn {
  opacity: 1;
}

.resize-handle {
  position: absolute;
  right: 0;
  top: 0;
  bottom: 0;
  width: 6px;
  cursor: col-resize;
  z-index: 10;
}

@media (pointer: coarse) {
  .resize-handle {
    width: 12px;
  }
}
</style>
