<script setup lang="ts">
defineProps<{
  fileName: string;
  hasChanges: boolean;
}>();
</script>

<template>
  <footer class="statusbar">
    <span>{{ fileName }}</span>
    <span v-if="hasChanges" class="unsaved">Unsaved changes</span>
  </footer>
</template>

<style scoped>
.statusbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 8px 20px;
  background: var(--el-bg-color-page);
  border-top: 1px solid var(--el-border-color);
  font-size: 12px;
  color: var(--el-text-color-regular);
}

.unsaved {
  color: var(--el-color-warning);
}
</style>
