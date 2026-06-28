<script setup lang="ts">
const { rowIndex } = defineProps<{
  rowIndex: number;
}>();

const emit = defineEmits<{
  (e: 'delete', index: number): void;
  (e: 'resize-start', event: MouseEvent | TouchEvent, index: number): void;
}>();

function handleDelete(index: number) {
  emit('delete', index);
}

function handleResizeStart(event: MouseEvent | TouchEvent) {
  emit('resize-start', event, rowIndex);
}
</script>

<template>
  <div class="row-number">
    <span>{{ rowIndex + 1 }}</span>
    <button class="delete-btn" @click.stop="handleDelete(rowIndex)">×</button>
    <div
      class="row-resize-handle"
      @mousedown.stop="handleResizeStart"
      @touchstart.stop="handleResizeStart"
    />
  </div>
</template>

<style scoped>
.row-number {
  position: relative;
  display: flex;
  align-items: center;
  justify-content: center;
  width: 100%;
  height: 100%;
  padding: 0 30px;
  text-align: center;
}

.row-resize-handle {
  position: absolute;
  left: 0;
  right: 0;
  bottom: 0;
  height: 10px;
  cursor: row-resize;
  z-index: 5;
  touch-action: none;
}

.delete-btn {
  opacity: 0;
  border: none;
  background: none;
  color: var(--el-color-danger);
  cursor: pointer;
  font-size: 16px;
  width: 28px;
  height: 28px;
  padding: 0;
  border-radius: 4px;
  transition: opacity 0.2s, background-color 0.2s;
  z-index: 6;
  position: absolute;
  right: 4px;
  top: 50%;
  transform: translateY(-50%);
}

.delete-btn:hover {
  background-color: var(--el-color-danger-light-9);
}

.row-number:hover .delete-btn {
  opacity: 1;
}

@media (pointer: coarse) {
  .row-number {
    padding: 0 30px 0 6px;
  }

  .delete-btn {
    opacity: 1;
    font-size: 18px;
  }

  .row-resize-handle {
    height: 18px;
  }
}
</style>
