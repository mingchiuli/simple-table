<script setup lang="ts">
import type { FileData } from '@/types';

defineProps<{
  fileData: FileData | null;
  showExport?: boolean;
  disabled?: boolean;
}>();

const emit = defineEmits<{
  (e: 'open-file'): void;
  (e: 'save-file'): void;
  (e: 'export-file'): void;
}>();
</script>

<template>
  <div class="file-buttons">
    <el-button type="primary" :disabled="disabled" @click="emit('open-file')">
      Open File
    </el-button>
    <el-button
      type="success"
      @click="emit('save-file')"
      :disabled="disabled || !fileData"
    >
      Save
    </el-button>
    <el-button
      v-if="showExport"
      @click="emit('export-file')"
      :disabled="disabled || !fileData"
    >
      Export
    </el-button>
  </div>
</template>

<style scoped>
.file-buttons {
  display: flex;
  gap: 8px;
  flex-shrink: 0;
}
</style>
