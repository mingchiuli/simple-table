<script setup lang="ts">
import { onMounted } from "vue";
import { useRouter } from "vue-router";
import { ElMessage } from "element-plus";
import { Document } from "@element-plus/icons-vue";
import type { FileData } from "@/types";
import { useFileDataStore } from "@/stores/fileData";
import { useRecentFilesStore } from "@/stores/recentFiles";
import { RecentFilesSection } from '@/components/file';
import * as api from "@/api";
import { openFile, getStorageType } from "@/platform";

const router = useRouter();
const fileDataStore = useFileDataStore();
const recentFilesStore = useRecentFilesStore();

onMounted(() => {
  recentFilesStore.load();
});

async function handleOpenFile() {
  try {
    const result = await openFile();
    if (!result) {
      // 用户取消选择
      return;
    }

    fileDataStore.set(result.fileData, result.path);

    const extension = result.fileName.split(".").pop() || "";
    const storageType = await getStorageType();
    await api.addRecentFileWithThumbnail(
      result.path,
      result.fileName,
      result.bytes?.length || 0,
      result.bytes || [],
      extension,
      storageType,
      result.originalPath
    );

    await recentFilesStore.load();
    router.push({ name: "table" });
  } catch (error) {
    ElMessage.error(`Failed to open file: ${error}`);
  }
}

async function handleNewFile() {
  const newFileData: FileData = {
    path: "",
    fileName: "untitled.xlsx",
    sheets: [
      {
        name: "Sheet1",
        rows: [
          [null, null, null, null, null],
          [null, null, null, null, null],
          [null, null, null, null, null],
          [null, null, null, null, null],
          [null, null, null, null, null],
        ],
        merges: [],
      },
    ],
  };

  await api.initFile(newFileData);
  fileDataStore.set(newFileData, null);
  router.push({ name: "table" });
}

function handleNavigate() {
  router.push({ name: "table" });
}
</script>

<template>
  <div class="home-view">
    <div v-if="recentFilesStore.files.length === 0" class="empty-state">
      <el-icon class="empty-icon"><Document /></el-icon>
      <p>No file opened</p>
      <div class="button-group">
        <el-button type="primary" @click="handleNewFile">
          New Table
        </el-button>
        <el-button @click="handleOpenFile">
          Open File
        </el-button>
      </div>
    </div>

    <RecentFilesSection v-else @open="handleNavigate">
      <template #actions>
        <div class="header-actions">
          <el-button @click="handleOpenFile">Open File</el-button>
          <el-button type="primary" @click="handleNewFile">New Table</el-button>
        </div>
      </template>
    </RecentFilesSection>
  </div>
</template>

<style scoped>
.home-view {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  min-height: 100dvh;
  min-height: 100vh;
  background-color: #fff;
  overflow-y: auto;
  -webkit-overflow-scrolling: touch;
  padding: 40px 20px;
}

.empty-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
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

.button-group {
  display: flex;
  gap: 12px;
}
</style>
