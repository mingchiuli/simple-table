<script setup lang="ts">
import { Close } from '@element-plus/icons-vue';

const props = defineProps<{
  columnIndex: number;
  title: string;
}>();

const emit = defineEmits<{
  (e: 'delete', index: number): void;
}>();

function handleDelete(index: number) {
  emit('delete', index);
}
</script>

<template>
  <div class="column-header">
    <span class="title">{{ title }}</span>
    <div class="actions">
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
  justify-content: center;
  width: 100%;
  height: 100%;
  padding: 0 30px;
  position: relative;
  text-align: center;
}

.title {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.actions {
  display: flex;
  gap: 2px;
  flex: 0 0 auto;
  position: absolute;
  right: 4px;
  top: 50%;
  transform: translateY(-50%);
}

.delete-btn {
  opacity: 0;
  border: none;
  background: none;
  cursor: pointer;
  padding: 2px;
  border-radius: 4px;
  width: 22px;
  height: 22px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  transition: opacity 0.2s, background-color 0.2s;
}

.delete-btn { color: var(--el-color-danger); }
.delete-btn:hover { background-color: var(--el-color-danger-light-9); }

.column-header:hover .delete-btn {
  opacity: 1;
}

@media (pointer: coarse) and (hover: none) {
  .column-header {
    padding: 0 34px 0 6px;
  }

  .delete-btn {
    opacity: 1;
    width: 28px;
    height: 28px;
  }
}
</style>
