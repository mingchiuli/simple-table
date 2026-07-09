<script setup lang="ts">
import { Document } from "@element-plus/icons-vue";
import { defaultRichProjection, type FileData, type RecentFile } from "@/types";
import { useDocumentSessionStore } from "@/stores/documentSession";
import { useRecentFilesStore } from "@/stores/recentFiles";
import { RecentFilesSection } from '@/components/file';
import * as api from "@/api";
import { pickOpenFile, readFile } from "@/platform";
import { blankCell } from "@/utils/cellValue";
import { warnRecentFileTrackingFailure } from "@/utils/recentFileTracking";
import { defaultSpreadsheetExtension } from "@/utils/spreadsheetFormats";
import { useDocumentLifecycle } from "@/composables/useDocumentLifecycle";
import { useDocumentReplacementGuard } from "@/composables/useDocumentReplacementGuard";
import { useOpenFileSelection } from "@/composables/useOpenFileSelection";
import { useRecentFileUpdates } from "@/composables/useRecentFileUpdates";

const router = useRouter();
const documentSessionStore = useDocumentSessionStore();
const recentFilesStore = useRecentFilesStore();
const { prepareForDocumentReplacement } = useDocumentReplacementGuard();
const { openSelectedFileOrDiscard } = useOpenFileSelection({
  prepareForDocumentReplacement,
});
const { runDocumentLifecycle } = useDocumentLifecycle();
const { queueRecentFileEntryUpdate, refreshRecentFiles } = useRecentFileUpdates();
const isBusy = ref(false);

onMounted(() => {
  void refreshRecentFiles();
});

async function runHomeFileAction(errorPrefix: string, action: () => Promise<void>) {
  if (isBusy.value) return;
  isBusy.value = true;
  try {
    await runDocumentLifecycle("loading", errorPrefix, action);
  } finally {
    isBusy.value = false;
  }
}

async function handleOpenFile() {
  await runHomeFileAction("Failed to open file", async () => {
    const selection = await pickOpenFile();
    if (!selection) {
      return;
    }
    const opened = await openSelectedFileOrDiscard(selection);
    if (!opened) return;
    documentSessionStore.openDocumentResponse(opened, selection.path);

    queueRecentFileEntryUpdate(selection.path, selection.fileName, selection.originalPath);
    await router.push({ name: "table" });
  });
}

async function handleNewFile() {
  await runHomeFileAction("Failed to create file", async () => {
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
  });
}

async function handleOpenRecent(file: RecentFile) {
  await runHomeFileAction("Failed to open file", async () => {
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
  const opened = await readFile(file.path);
  documentSessionStore.openDocumentResponse(opened, file.path);
  queueRecentFileEntryUpdate(file.path, file.fileName, file.originalPath);
  return true;
}

async function relocateAndOpenRecent(file: RecentFile): Promise<boolean> {
  const selection = await pickOpenFile();
  if (!selection) return false;
  const opened = await openSelectedFileOrDiscard(selection);
  if (!opened) return false;
  documentSessionStore.openDocumentResponse(opened, selection.path);
  queueRecentFileEntryUpdate(selection.path, selection.fileName, selection.originalPath);

  if (file.path !== selection.path) {
    try {
      await api.removeRecentFile(file.id);
    } catch (error) {
      warnRecentFileTrackingFailure(error);
    }
  }
  void refreshRecentFiles();
  return true;
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
