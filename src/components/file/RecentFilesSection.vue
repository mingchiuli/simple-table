<script setup lang="ts">
import { Document, Delete } from "@element-plus/icons-vue";
import { ElMessage } from "element-plus";
import type { RecentFile } from "@/types";
import { useRecentFilesStore } from "@/stores/recentFiles";
import { appErrorMessage } from "@/utils/appError";

const recentFilesStore = useRecentFilesStore();

const props = defineProps<{
  disabled?: boolean;
}>();

const emit = defineEmits<{
  (e: "open", file: RecentFile): void;
}>();

function handleOpenRecent(file: RecentFile) {
  if (props.disabled) return;
  emit("open", file);
}

async function handleDeleteRecent(id: string, event: Event) {
  event.stopPropagation();
  if (props.disabled) return;
  try {
    await recentFilesStore.remove(id);
  } catch (error) {
    ElMessage.error(`Failed to remove recent file: ${appErrorMessage(error)}`);
  }
}

function formatFileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function formatDate(timestamp: number): string {
  const date = new Date(timestamp);
  const now = new Date();
  const diff = now.getTime() - date.getTime();
  const days = Math.floor(diff / (1000 * 60 * 60 * 24));

  if (days === 0) return "Today";
  if (days === 1) return "Yesterday";
  if (days < 7) return `${days} days ago`;
  return date.toLocaleDateString();
}
</script>

<template>
  <div class="recent-section">
    <div class="recent-header">
      <h3>Recent Files</h3>
      <slot name="actions" />
    </div>

    <div class="recent-grid">
      <div
        v-for="file in recentFilesStore.files"
        :key="file.id"
        :class="['recent-card', { disabled: props.disabled }]"
        @click="handleOpenRecent(file)"
      >
        <div class="thumbnail">
          <el-image
            v-if="file.thumbnail"
            :src="file.thumbnail"
            fit="cover"
            class="thumbnail-img"
          >
            <template #error>
              <el-icon size="48"><Document /></el-icon>
            </template>
          </el-image>
          <el-icon v-else size="48"><Document /></el-icon>
        </div>
        <div class="info">
          <div class="filename" :title="file.path">{{ file.fileName }}</div>
          <div class="meta">
            {{ formatFileSize(file.fileSize) }} · {{ formatDate(file.lastOpened) }}
          </div>
        </div>
        <el-icon class="delete-btn" @click="handleDeleteRecent(file.id, $event)">
          <Delete />
        </el-icon>
      </div>
    </div>
  </div>
</template>

<style scoped>
.recent-section {
  width: 100%;
  max-width: 900px;
}

.recent-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 24px;
}

.recent-header h3 {
  margin: 0;
  font-size: 20px;
  font-weight: 600;
  color: var(--el-text-color-primary);
}

.recent-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
  gap: 20px;
}

.recent-card {
  position: relative;
  display: flex;
  flex-direction: column;
  background: var(--el-bg-color);
  border: 1px solid var(--el-border-color-light);
  border-radius: 8px;
  overflow: hidden;
  cursor: pointer;
  transition: all 0.2s ease;
}

.recent-card:hover {
  border-color: var(--el-color-primary);
  box-shadow: 0 2px 12px rgba(64, 158, 255, 0.15);
}

.recent-card.disabled {
  cursor: not-allowed;
  opacity: 0.65;
}

.recent-card.disabled:hover {
  border-color: var(--el-border-color-light);
  box-shadow: none;
}

.thumbnail {
  width: 100%;
  height: 140px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--el-bg-color-page);
  color: var(--el-text-color-secondary);
}

.thumbnail-img {
  width: 100%;
  height: 100%;
}

.info {
  padding: 12px;
}

.filename {
  font-size: 14px;
  font-weight: 500;
  color: var(--el-text-color-primary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  margin-bottom: 4px;
}

.meta {
  font-size: 12px;
  color: var(--el-text-color-secondary);
}

.delete-btn {
  position: absolute;
  top: 8px;
  right: 8px;
  padding: 4px;
  background: rgba(255, 255, 255, 0.9);
  border-radius: 4px;
  color: var(--el-text-color-secondary);
  opacity: 0;
  transition: all 0.2s ease;
}

.recent-card:hover .delete-btn {
  opacity: 1;
}

.delete-btn:hover {
  color: var(--el-color-danger);
}

/* 移动端：总是显示删除按钮 */
@media (hover: none) and (pointer: coarse) {
  .delete-btn {
    opacity: 1;
    background: rgba(255, 255, 255, 0.7);
  }

  .recent-card:active {
    border-color: var(--el-color-primary);
    box-shadow: 0 2px 12px rgba(64, 158, 255, 0.15);
  }
}

/* 移动端卡片网格调整 */
@media (max-width: 480px) {
  .recent-grid {
    grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
    gap: 12px;
  }

  .thumbnail {
    height: 100px;
  }

  .recent-header {
    flex-direction: column;
    align-items: flex-start;
    gap: 12px;
  }
}
</style>
