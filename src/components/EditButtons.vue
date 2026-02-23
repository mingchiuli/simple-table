<script setup lang="ts">
defineProps<{
  canUndo: boolean;
  canRedo: boolean;
  columnCount: number;
}>();

const emit = defineEmits<{
  (e: 'undo'): void;
  (e: 'redo'): void;
  (e: 'add-row'): void;
  (e: 'add-column'): void;
  (e: 'delete-column'): void;
}>();
</script>

<template>
  <div class="edit-buttons">
    <el-button @click="emit('undo')" :disabled="!canUndo">
      Undo
    </el-button>
    <el-button @click="emit('redo')" :disabled="!canRedo">
      Redo
    </el-button>
    <el-button @click="emit('add-row')">+ Row</el-button>
    <el-button @click="emit('add-column')">+ Column</el-button>
    <el-button @click="emit('delete-column')" :disabled="columnCount <= 1">
      - Column
    </el-button>
  </div>
</template>

<style scoped>
.edit-buttons {
  display: flex;
  gap: 8px;
  flex-shrink: 0;
}
</style>
