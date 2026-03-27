<script setup lang="ts">
import { Close } from "@element-plus/icons-vue";
import type { SearchResult } from "@/types";

defineProps<{
  results: SearchResult[];
  query: string;
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

// 高亮显示查询词在文本中的匹配，文本过长时以匹配词为中心截断
function getHighlightedSnippet(text: string, query: string, maxLen: number = 10): string {
  if (!query) return text;

  const pos = text.toLowerCase().indexOf(query.toLowerCase());

  // 文本足够短或没找到匹配，返回完整高亮
  if (pos === -1 || text.length <= maxLen) {
    const regex = new RegExp(`(${query})`, "gi");
    return text.replace(regex, '<mark>$1</mark>');
  }

  // 计算截断范围，以匹配词为中心
  const half = Math.floor((maxLen - query.length) / 2);
  let start = Math.max(0, pos - half);
  let end = Math.min(text.length, pos + query.length + half);

  // 边界调整
  if (start > 0) end = Math.min(text.length, start + maxLen);
  if (end < text.length) start = Math.max(0, end - maxLen);

  const snippet = (start > 0 ? '...' : '') + text.slice(start, end) + (end < text.length ? '...' : '');
  return snippet.replace(new RegExp(`(${query})`, "gi"), '<mark>$1</mark>');
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
        <span class="cell-position">{{ result.cellPosition }}</span>
        <span class="cell-value" v-html="getHighlightedSnippet(result.value, query)"></span>
        <span v-if="result.sheetName" class="sheet-name">{{ result.sheetName }}</span>
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

.cell-value :deep(mark) {
  background-color: #fef0f0;
  color: #f56c6c;
  padding: 0 2px;
  border-radius: 2px;
}

.sheet-name {
  font-size: 12px;
  color: #909399;
}
</style>
