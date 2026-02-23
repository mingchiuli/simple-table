<script setup lang="ts">
import { Close } from "@element-plus/icons-vue";
import type { SearchResult } from "@/types";

defineProps<{
  results: SearchResult[];
}>();

const emit = defineEmits<{
  (e: "result-click", result: SearchResult): void;
  (e: "clear"): void;
}>();

function handleResultClick(result: SearchResult) {
  emit("result-click", result);
}

function handleClear() {
  emit("clear");
}
</script>

<template>
  <div v-if="results.length > 0" class="search-panel">
    <div class="search-panel-header">
      <span>Found {{ results.length }} result(s)</span>
      <el-button text @click="handleClear">
        <el-icon><Close /></el-icon>
      </el-button>
    </div>
    <div class="search-panel-list">
      <div
        v-for="(result, index) in results"
        :key="index"
        class="search-result-item"
        @click="handleResultClick(result)"
      >
        <span class="cell-position">{{ result.cell_position }}</span>
        <span class="cell-value">{{ result.value }}</span>
        <span v-if="result.sheet_name" class="sheet-name">{{ result.sheet_name }}</span>
      </div>
    </div>
  </div>
</template>

<style scoped>
.search-panel {
  width: 280px;
  background: #fff;
  border-left: 1px solid #e4e7ed;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.search-panel-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 12px;
  border-bottom: 1px solid #e4e7ed;
  font-size: 14px;
  color: #606266;
}

.search-panel-list {
  flex: 1;
  overflow-y: auto;
}

.search-result-item {
  display: flex;
  align-items: center;
  padding: 10px 12px;
  cursor: pointer;
  gap: 8px;
  border-bottom: 1px solid #f0f0f0;
}

.search-result-item:hover {
  background: #f5f7fa;
}

.cell-position {
  font-weight: bold;
  color: #409eff;
  min-width: 40px;
}

.cell-value {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.sheet-name {
  font-size: 12px;
  color: #909399;
}
</style>
