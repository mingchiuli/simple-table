<script setup lang="ts">
defineProps<{
  rowIndex: number;
}>();

const emit = defineEmits<{
  (e: 'delete', index: number): void;
}>();

function handleDelete(index: number) {
  emit('delete', index);
}
</script>

<template>
  <div class="row-number">
    <span>{{ rowIndex + 1 }}</span>
    <button class="delete-btn" @click.stop="handleDelete(rowIndex)">×</button>
  </div>
</template>

<style scoped>
.row-number {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}

.delete-btn {
  opacity: 0;
  border: none;
  background: none;
  color: #f56c6c;
  cursor: pointer;
  font-size: 14px;
  transition: opacity 0.2s;
}

:deep(.el-table-v2__row:hover) .delete-btn {
  opacity: 1;
}
</style>
