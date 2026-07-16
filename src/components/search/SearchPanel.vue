<script setup lang="ts">
import { Close } from "@element-plus/icons-vue";
import type { SearchResult } from "@/types";

const props = defineProps<{
  results: SearchResult[];
  query: string;
  truncated?: boolean;
  disabled?: boolean;
}>();

const emit = defineEmits<{
  (e: "result-click", result: SearchResult): void;
  (e: "clear"): void;
}>();

function handleResultClick(result: SearchResult) {
  if (props.disabled) return;
  emit("result-click", result);
}

function handleClear() {
  emit("clear");
}

// 转义 HTML 特殊字符，防止 XSS
function escapeHtml(text: string): string {
  const div = document.createElement('div');
  div.textContent = text;
  return div.innerHTML;
}

// 转义正则特殊字符
function escapeRegex(text: string): string {
  return text.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

// 高亮显示查询词在文本中的匹配，文本过长时以匹配词为中心截断
function getHighlightedSnippet(text: string, query: string, maxLen: number = 10): string {
  if (!query) return escapeHtml(text);

  // 转义正则特殊字符，防止 regex 错误
  const escapedQuery = escapeRegex(query);
  const lowerText = text.toLowerCase();
  const lowerQuery = escapedQuery.toLowerCase();

  const pos = lowerText.indexOf(lowerQuery);

  // 文本足够短或没找到匹配，返回完整高亮
  if (pos === -1 || text.length <= maxLen) {
    const regex = new RegExp(`(${escapedQuery})`, "gi");
    return escapeHtml(text).replace(regex, '<mark>$1</mark>');
  }

  // 计算截断范围，以匹配词为中心
  const half = Math.floor((maxLen - query.length) / 2);
  let start = Math.max(0, pos - half);
  let end = Math.min(text.length, pos + query.length + half);

  // 边界调整
  if (start > 0) end = Math.min(text.length, start + maxLen);
  if (end < text.length) start = Math.max(0, end - maxLen);

  const snippet = (start > 0 ? '...' : '') + text.slice(start, end) + (end < text.length ? '...' : '');
  return escapeHtml(snippet).replace(new RegExp(`(${escapedQuery})`, "gi"), '<mark>$1</mark>');
}
</script>

<template>
  <div v-if="props.results.length > 0" class="search-panel">
    <div class="search-panel-header">
      <span>Found {{ props.results.length }}{{ props.truncated ? "+" : "" }} result(s)</span>
      <el-button text @click="handleClear">
        <el-icon><Close /></el-icon>
      </el-button>
    </div>
    <div class="search-panel-list">
      <div
        v-for="(result, index) in props.results"
        :key="index"
        :class="['search-result-item', { disabled: props.disabled }]"
        @click="handleResultClick(result)"
      >
        <span class="cell-position">{{ result.cellPosition }}</span>
        <span class="cell-value" v-html="getHighlightedSnippet(result.value, props.query)"></span>
        <span v-if="result.sheetName" class="sheet-name">{{ result.sheetName }}</span>
      </div>
    </div>
  </div>
</template>

<style scoped>
.search-panel {
  width: 280px;
  min-width: 280px;
  background: var(--el-bg-color);
  border-left: 1px solid var(--el-border-color);
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.search-panel-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 12px;
  border-bottom: 1px solid var(--el-border-color);
  font-size: 14px;
  color: var(--el-text-color-regular);
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
  border-bottom: 1px solid var(--el-border-color-lighter);
}

.search-result-item:hover {
  background: var(--el-bg-color-page);
}

.search-result-item.disabled {
  cursor: not-allowed;
  opacity: 0.65;
}

.search-result-item.disabled:hover {
  background: transparent;
}

.cell-position {
  font-weight: bold;
  color: var(--el-color-primary);
  min-width: 40px;
}

.cell-value {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.cell-value :deep(mark) {
  background-color: var(--el-color-danger-light-9);
  color: var(--el-color-danger);
  padding: 0 2px;
  border-radius: 2px;
}

.sheet-name {
  font-size: 12px;
  color: var(--el-text-color-secondary);
}

@media (max-width: 900px), (pointer: coarse) {
  .search-panel {
    width: auto;
    min-width: 0;
    max-width: none;
    border-left: none;
    background: var(--el-bg-color-overlay);
  }

  .search-panel-header {
    padding: 10px 12px;
  }

  .search-panel-list {
    max-height: calc(42vh - 44px);
    -webkit-overflow-scrolling: touch;
  }

  .search-result-item {
    min-height: 44px;
    padding: 10px 12px;
  }

  .cell-value {
    white-space: normal;
    overflow-wrap: anywhere;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    -webkit-box-orient: vertical;
  }
}
</style>
