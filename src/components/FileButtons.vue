<script setup lang="ts">
import type { FileData } from '@/types';

defineProps<{
  fileData: FileData | null;
}>();

const emit = defineEmits<{
  (e: 'open-file'): void;
  (e: 'save-file'): void;
}>();
</script>

<template>
  <div class="file-buttons">
    <el-button type="primary" @click="emit('open-file')">
      Open File
    </el-button>
    <el-button
      type="success"
      @click="emit('save-file')"
      :disabled="!fileData"
    >
      Save
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
