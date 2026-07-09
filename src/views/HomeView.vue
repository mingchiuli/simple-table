<script setup lang="ts">
import { Document } from "@element-plus/icons-vue";
import { defaultRichProjection, type FileData } from "@/types";
import { useDocumentSessionStore } from "@/stores/documentSession";
import { useRecentFilesStore } from "@/stores/recentFiles";
import { RecentFilesSection } from '@/components/file';
import * as api from "@/api";
import { openFile, getStorageType } from "@/platform";
import { blankCell } from "@/utils/cellValue";
import { DEFAULT_SPREADSHEET_EXTENSION } from "@/utils/fileFormats";

const router = useRouter();
const documentSessionStore = useDocumentSessionStore();
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

    documentSessionStore.openDocumentResponse(result, result.path);

    const storageType = await getStorageType();
    const fileSize = await api.getFileSize(result.path);
    await api.addRecentFileWithThumbnail(
      result.path,
      result.fileName,
      fileSize,
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
    fileName: `untitled.${DEFAULT_SPREADSHEET_EXTENSION}`,
    sheets: [
      {
        name: "Sheet1",
        rows: [
          [blankCell(), blankCell(), blankCell(), blankCell(), blankCell()],
          [blankCell(), blankCell(), blankCell(), blankCell(), blankCell()],
          [blankCell(), blankCell(), blankCell(), blankCell(), blankCell()],
          [blankCell(), blankCell(), blankCell(), blankCell(), blankCell()],
          [blankCell(), blankCell(), blankCell(), blankCell(), blankCell()],
        ],
        merges: [],
        rich: defaultRichProjection(),
      },
    ],
  };

  const opened = await api.initFile(newFileData);
  documentSessionStore.openDocumentResponse(opened, null);
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
  width: 100%;
  height: 100%;
  min-height: 0;
  background-color: var(--el-bg-color);
  overflow-y: auto;
  -webkit-overflow-scrolling: touch;
  padding: 40px 20px max(40px, env(safe-area-inset-bottom));
}

.empty-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  color: var(--el-text-color-secondary);
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

@media (max-width: 480px) {
  .home-view {
    justify-content: flex-start;
    padding: 24px 16px max(24px, env(safe-area-inset-bottom));
  }

  .button-group {
    width: 100%;
    flex-direction: column;
  }

  .button-group :deep(.el-button) {
    width: 100%;
    margin-left: 0;
  }
}
</style>
