<script setup lang="ts">
defineProps<{
  sheetCount: number;
  canInsertDeleteSheets: boolean;
  disabled?: boolean;
}>();

const emit = defineEmits<{
  (e: 'add-sheet'): void;
  (e: 'delete-sheet'): void;
}>();
</script>

<template>
  <div class="sheet-buttons">
    <el-button
      size="small"
      :disabled="disabled || !canInsertDeleteSheets"
      @click="emit('add-sheet')"
    >
      + Sheet
    </el-button>
    <el-button
      size="small"
      type="danger"
      :disabled="disabled || sheetCount <= 1 || !canInsertDeleteSheets"
      @click="emit('delete-sheet')"
    >
      - Sheet
    </el-button>
  </div>
</template>

<style scoped>
.sheet-buttons {
  display: flex;
  gap: 8px;
}
</style>
