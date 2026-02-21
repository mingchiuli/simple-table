<script setup lang="ts">
const emit = defineEmits<{
  (e: 'open-file'): void;
}>();
</script>

<template>
  <div class="empty-state">
    <el-icon class="empty-icon"><Document /></el-icon>
    <p>No file opened</p>
    <el-button type="primary" @click="emit('open-file')">
      Open Excel or CSV file
    </el-button>
  </div>
</template>

<style scoped>
.empty-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  height: 100%;
  color: #909399;
}

.empty-icon {
  font-size: 64px;
  margin-bottom: 16px;
}

.empty-state p {
  font-size: 16px;
  margin-bottom: 20px;
}
</style>
