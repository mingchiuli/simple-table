<script setup lang="ts">
import { Close } from '@element-plus/icons-vue';
import { usePlatform } from '@/composables/usePlatform';

const { isTouchDevice } = usePlatform();

const props = defineProps<{
  columnIndex: number;
  title: string;
  width?: number;
}>();

const emit = defineEmits<{
  (e: 'delete', index: number): void;
  (e: 'resize-start', event: MouseEvent | TouchEvent, colIndex: number): void;
}>();

function handleDelete(index: number) {
  emit('delete', index);
}

// 处理 resize 事件
function handleResizeStart(e: MouseEvent | TouchEvent) {
  emit('resize-start', e, props.columnIndex);
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
  flex: 0 0 auto;
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
  .column-header {
    padding: 0 2px 0 4px;
  }

  .delete-btn {
    opacity: 1;
    width: 28px;
    height: 28px;
  }

  .resize-handle {
    right: -4px;
    width: 18px;
  }
}
</style>
