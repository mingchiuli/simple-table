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

.row-number:hover .delete-btn {
  opacity: 1;
}
</style>
