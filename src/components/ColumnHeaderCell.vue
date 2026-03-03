<script setup lang="ts">
import { computed } from 'vue';
import type { SortState } from '@/types';

const props = defineProps<{
  columnIndex: number;
  title: string;
  sortState?: SortState | null;
}>();

const emit = defineEmits<{
  (e: 'delete', index: number): void;
  (e: 'sort', ascending: boolean): void;
}>();

function handleDelete(index: number) {
  emit('delete', index);
}

function handleSort() {
  const isCurrentColumn = props.sortState !== null && props.sortState?.col_index === props.columnIndex;
  // 如果当前列已排序，切换升序/降序；否则默认升序
  const newAscending = isCurrentColumn && props.sortState ? !props.sortState.ascending : true;
  emit('sort', newAscending);
}

// 判断当前列是否正在排序
const isCurrentSorting = computed(() => props.sortState?.col_index === props.columnIndex);

// 排序方向图标
const sortIcon = computed(() => {
  if (!isCurrentSorting.value) return '↕';
  const sortState = props.sortState;
  return sortState && sortState.ascending ? '↑' : '↓';
});
</script>

<template>
  <div class="column-header">
    <span class="title">{{ title }}</span>
    <div class="actions">
      <button
        class="sort-btn"
        :class="{ active: isCurrentSorting }"
        :title="isCurrentSorting && sortState?.ascending ? '降序排列' : '升序排列'"
        @click.stop="handleSort"
      >
        {{ sortIcon }}
      </button>
      <button class="delete-btn" @click.stop="handleDelete(columnIndex)">×</button>
    </div>
  </div>
</template>

<style scoped>
.column-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  width: 100%;
  height: 100%;
  padding: 0 4px;
}

.title {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.actions {
  display: flex;
  align-items: center;
  gap: 4px;
}

.sort-btn {
  opacity: 0;
  border: none;
  background: none;
  color: #409eff;
  cursor: pointer;
  font-size: 12px;
  padding: 2px 4px;
  border-radius: 4px;
  transition: opacity 0.2s, background-color 0.2s;
}

.sort-btn:hover {
  background-color: #ecf5ff;
}

.sort-btn.active {
  opacity: 1;
  color: #409eff;
  font-weight: bold;
}

.delete-btn {
  opacity: 0;
  border: none;
  background: none;
  color: #f56c6c;
  cursor: pointer;
  font-size: 16px;
  padding: 2px 6px;
  border-radius: 4px;
  transition: opacity 0.2s, background-color 0.2s;
}

.delete-btn:hover {
  background-color: #fef0f0;
}

.column-header:hover .delete-btn,
.column-header:hover .sort-btn {
  opacity: 1;
}
</style>
