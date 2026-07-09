<script setup lang="ts">
import { Document } from "@element-plus/icons-vue";
import { defaultRichProjection, type FileData, type RecentFile } from "@/types";
import { useDocumentSessionStore } from "@/stores/documentSession";
import { useRecentFilesStore } from "@/stores/recentFiles";
import { RecentFilesSection } from '@/components/file';
import * as api from "@/api";
import { pickOpenFile, readFile, getStorageType } from "@/platform";
import { blankCell } from "@/utils/cellValue";
import {
  tryAddRecentFileWithResolvedStorage,
  tryRefreshRecentFiles,
  warnRecentFileTrackingFailure,
} from "@/utils/recentFileTracking";
import { defaultSpreadsheetExtension } from "@/utils/spreadsheetFormats";
import { useDocumentReplacementGuard } from "@/composables/useDocumentReplacementGuard";

const router = useRouter();
const documentSessionStore = useDocumentSessionStore();
const recentFilesStore = useRecentFilesStore();
const { prepareForDocumentReplacement } = useDocumentReplacementGuard();
const isBusy = ref(false);

onMounted(() => {
  recentFilesStore.load();
});

async function runHomeFileAction(action: () => Promise<void>) {
  if (isBusy.value || !documentSessionStore.beginLifecycle("loading")) return;
  isBusy.value = true;
  try {
    await action();
  } finally {
    isBusy.value = false;
    documentSessionStore.endLifecycle("loading");
  }
}

async function trackOpenedFile(
  path: string,
  fileName: string,
  originalPath?: string
) {
  await tryAddRecentFileWithResolvedStorage(
    {
      path,
      fileName,
      originalPath,
      context: documentSessionStore.currentCommandContext(),
    },
    getStorageType
  );
  await tryRefreshRecentFiles(() => recentFilesStore.load());
}

async function handleOpenFile() {
  await runHomeFileAction(async () => {
    try {
      const selection = await pickOpenFile();
      if (!selection) {
        // 用户取消选择
        return;
      }
      if (!(await prepareForDocumentReplacement())) return;

      const opened = await readFile(selection.path);
      documentSessionStore.openDocumentResponse(opened, selection.path);

      await trackOpenedFile(selection.path, selection.fileName, selection.originalPath);
      await router.push({ name: "table" });
    } catch (error) {
      ElMessage.error(`Failed to open file: ${error}`);
    }
  });
}

async function handleNewFile() {
  await runHomeFileAction(async () => {
    try {
      if (!(await prepareForDocumentReplacement())) return;
      const defaultExtension = await defaultSpreadsheetExtension();
      const newFileData: FileData = {
        path: "",
        fileName: `untitled.${defaultExtension}`,
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
      await router.push({ name: "table" });
    } catch (error) {
      ElMessage.error(`Failed to create file: ${error}`);
    }
  });
}

async function handleOpenRecent(file: RecentFile) {
  await runHomeFileAction(async () => {
    if (await api.checkFileExists(file.path)) {
      if (!(await prepareForDocumentReplacement())) return;
      if (await openRecentPath(file)) {
        await router.push({ name: "table" });
      }
      return;
    }

    if (await relocateAndOpenRecent(file)) {
      await router.push({ name: "table" });
    }
  });
}

async function openRecentPath(file: RecentFile): Promise<boolean> {
  try {
    const opened = await readFile(file.path);
    documentSessionStore.openDocumentResponse(opened, file.path);
    await trackOpenedFile(file.path, file.fileName, file.originalPath);
    return true;
  } catch (error) {
    ElMessage.error(`Failed to open file: ${error}`);
    return false;
  }
}

async function relocateAndOpenRecent(file: RecentFile): Promise<boolean> {
  try {
    const selection = await pickOpenFile();
    if (!selection) return false;
    if (!(await prepareForDocumentReplacement())) return false;

    const opened = await readFile(selection.path);
    documentSessionStore.openDocumentResponse(opened, selection.path);
    await trackOpenedFile(selection.path, selection.fileName, selection.originalPath);

    if (file.path !== selection.path) {
      try {
        await api.removeRecentFile(file.id);
      } catch (error) {
        warnRecentFileTrackingFailure(error);
      }
    }
    await tryRefreshRecentFiles(() => recentFilesStore.load());
    return true;
  } catch (error) {
    ElMessage.error(`Failed to open file: ${error}`);
    return false;
  }
}
</script>

<template>
  <div class="home-view">
    <div v-if="recentFilesStore.files.length === 0" class="empty-state">
      <el-icon class="empty-icon"><Document /></el-icon>
      <p>No file opened</p>
      <div class="button-group">
        <el-button type="primary" :disabled="isBusy" @click="handleNewFile">
          New Table
        </el-button>
        <el-button :disabled="isBusy" @click="handleOpenFile">
          Open File
        </el-button>
      </div>
    </div>

    <RecentFilesSection v-else :disabled="isBusy" @open="handleOpenRecent">
      <template #actions>
        <div class="header-actions">
          <el-button :disabled="isBusy" @click="handleOpenFile">Open File</el-button>
          <el-button type="primary" :disabled="isBusy" @click="handleNewFile">New Table</el-button>
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
