<script setup lang="ts">
import { computed } from 'vue';
import { Sort, Close } from '@element-plus/icons-vue';
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
        <el-icon :size="12"><Sort /></el-icon>
      </button>
      <button class="delete-btn" @click.stop="handleDelete(columnIndex)">
        <el-icon :size="12"><Close /></el-icon>
      </button>
    </div>
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
</style>
