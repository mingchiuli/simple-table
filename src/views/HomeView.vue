<script setup lang="ts">
import { onMounted } from "vue";
import { useRouter } from "vue-router";
import { open } from "@tauri-apps/plugin-dialog";
import { readFile } from "@tauri-apps/plugin-fs";
import { basename } from "@tauri-apps/api/path";
import { ElMessage } from "element-plus";
import { Document } from "@element-plus/icons-vue";
import type { FileData } from "@/types";
import { useFileDataStore } from "@/stores/fileData";
import { useRecentFilesStore } from "@/stores/recentFiles";
import RecentFilesSection from "@/components/RecentFilesSection.vue";
import * as api from "@/api";
import { isAndroid } from "@/utils/platform";

const router = useRouter();
const fileDataStore = useFileDataStore();
const recentFilesStore = useRecentFilesStore();

onMounted(() => {
  recentFilesStore.load();
});

async function handleOpenFile() {
  try {
    // Android: 使用专用文件选择器
    if (await isAndroid()) {
      const result = await api.pickFileAndroid();
      const fileData = await api.readFileBytes(result.path, result.bytes);
      fileDataStore.set(fileData);

      const extension = result.fileName.split(".").pop() || "";
      await api.addRecentFileWithThumbnail(
        result.path,
        result.fileName,
        result.bytes.length,
        result.bytes,
        extension,
        'androidUri'
      );

      await recentFilesStore.load();
      router.push({ name: "table" });
      return;
    }

    // 桌面端/iOS: 使用标准文件选择器
    const selected = await open({
      multiple: false,
      filters: [
        {
          name: "Spreadsheet",
          extensions: ["xlsx", "xls", "csv", "ods"],
        },
      ],
    });

    if (selected) {
      const bytes = await readFile(selected);
      const bytesArray = Array.from(bytes);
      const result = await api.readFileBytes(selected, bytesArray);
      fileDataStore.set(result);

      // 使用 bytes 版本添加最近文件（桌面端默认类型）
      const fileName = await basename(selected);
      const extension = fileName.split(".").pop() || "";
      await api.addRecentFileWithThumbnail(selected, fileName, bytes.byteLength, bytesArray, extension);

      await recentFilesStore.load();
      router.push({ name: "table" });
    }
  } catch (error) {
    ElMessage.error(`Failed to open file: ${error}`);
  }
}

async function handleNewFile() {
  const newFileData: FileData = {
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
  fileDataStore.set(newFileData);
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
