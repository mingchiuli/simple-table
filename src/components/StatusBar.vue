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
  background: #f5f7fa;
  border-top: 1px solid #e4e7ed;
  font-size: 12px;
  color: #606266;
}

.unsaved {
  color: #e6a23c;
}
</style>
