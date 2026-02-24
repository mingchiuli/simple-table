<script setup lang="ts">
defineProps<{
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
    <span>{{ title }}</span>
    <button class="delete-btn" @click.stop="handleDelete(columnIndex)">×</button>
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

.column-header:hover .delete-btn {
  opacity: 1;
}
</style>
