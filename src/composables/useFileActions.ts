import type { ComputedRef, Ref } from 'vue';
import * as api from '@/api';
import { exportFile, getFileName, getStorageType, openFile, pickSaveLocation, readFile, saveFile } from '@/platform';
import { useDocumentSessionStore } from '@/stores/documentSession';
import { useRecentFilesStore } from '@/stores/recentFiles';
import type { FileData } from '@/types';
import { documentCapabilities, nativeSavePlan } from '@/utils/documentCapabilities';
import { baseNameWithoutExtension, isUntitledSpreadsheet } from '@/utils/fileFormats';
import {
  tryAddRecentFileWithResolvedStorage,
  tryRefreshRecentFiles,
} from '@/utils/recentFileTracking';
import { defaultSpreadsheetExtension } from '@/utils/spreadsheetFormats';

type UseFileActionsOptions = {
  fileData: ComputedRef<FileData | null>;
  currentSheetIndex: Ref<number>;
  isLoading: Ref<boolean>;
  isFileLoading: Ref<boolean>;
  flushPendingCellChanges: () => Promise<boolean>;
  resetDocumentStatus: () => void;
};

export function useFileActions({
  fileData,
  currentSheetIndex,
  isLoading,
  isFileLoading,
  flushPendingCellChanges,
  resetDocumentStatus,
}: UseFileActionsOptions) {
  const router = useRouter();
  const documentSessionStore = useDocumentSessionStore();
  const recentFilesStore = useRecentFilesStore();

  async function withDocumentLifecycle(
    lifecycle: 'loading' | 'saving',
    errorPrefix: string,
    action: () => Promise<void>
  ) {
    try {
      documentSessionStore.beginLifecycle(lifecycle);
      await action();
    } catch (error) {
      ElMessage.error(`${errorPrefix}: ${error}`);
    } finally {
      documentSessionStore.endLifecycle(lifecycle);
    }
  }

  async function updateRecentFileEntry(
    path: string,
    fileName: string,
    originalPath?: string
  ) {
    if (!fileData.value) return;

    await tryAddRecentFileWithResolvedStorage({ path, fileName, originalPath }, getStorageType);
    await tryRefreshRecentFiles(() => recentFilesStore.load());
  }

  async function loadFileFromPath(filePath: string) {
    await withDocumentLifecycle('loading', 'Failed to open file', async () => {
      isLoading.value = true;
      isFileLoading.value = true;
      if (!(await flushPendingCellChanges())) return;
      await documentSessionStore.waitForMutations();

      const opened = await readFile(filePath);
      documentSessionStore.openDocumentResponse(opened, filePath);
      currentSheetIndex.value = 0;

      const fileName = await getFileName(filePath);
      await updateRecentFileEntry(filePath, fileName);
    });
    isLoading.value = false;
    isFileLoading.value = false;
  }

  async function handleOpenFile() {
    await withDocumentLifecycle('loading', 'Failed to open file', async () => {
      isLoading.value = true;
      isFileLoading.value = true;
      if (!(await flushPendingCellChanges())) return;
      await documentSessionStore.waitForMutations();

      const result = await openFile();
      if (!result) return;

      documentSessionStore.openDocumentResponse(result, result.path);
      currentSheetIndex.value = 0;

      await updateRecentFileEntry(result.path, result.fileName, result.originalPath);
    });
    isLoading.value = false;
    isFileLoading.value = false;
  }

  async function handleSaveFile() {
    const data = fileData.value;
    if (!data) return;

    await withDocumentLifecycle('saving', 'Failed to save file', async () => {
      if (!(await flushPendingCellChanges())) return;
      await documentSessionStore.waitForMutations();

      const isNewFile = isUntitledSpreadsheet(data.fileName);
      const defaultName = isNewFile ? 'untitled' : baseNameWithoutExtension(data.fileName);

      const existingPath = documentSessionStore.currentFilePath;
      const existingTarget = existingPath ?? data.fileName;
      const savePlan = await nativeSavePlan(existingTarget);

      if (existingPath && savePlan.canSave && !savePlan.requiresSaveAs) {
        isLoading.value = true;
        const saved = await saveFile(existingPath);
        documentSessionStore.applySavedDocumentResponse(saved, existingPath);
        const fileName = saved.fileData.fileName || await getFileName(existingPath);
        await updateRecentFileEntry(existingPath, fileName);
        ElMessage.success('File saved successfully');
        return;
      }

      if (existingPath && !savePlan.requiresSaveAs && !savePlan.canSave) {
        ElMessage.error(savePlan.blockedReason ?? 'Workbook cannot be saved in its current state.');
        return;
      }

      const fallbackExtension = savePlan.defaultExtension;
      const savePath = await pickSaveLocation(`${defaultName}.${fallbackExtension}`);
      if (!savePath) return;

      const targetPlan = await nativeSavePlan(savePath);
      if (!targetPlan.canSave) {
        ElMessage.error(targetPlan.blockedReason ?? 'Workbook cannot be saved in its current state.');
        return;
      }

      isLoading.value = true;
      const saved = await saveFile(savePath);
      documentSessionStore.applySavedDocumentResponse(saved, savePath);
      const fileName = saved.fileData.fileName || await getFileName(savePath);

      await updateRecentFileEntry(savePath, fileName);
      ElMessage.success('File saved successfully');
    });
    isLoading.value = false;
  }

  async function handleExportFile() {
    const data = fileData.value;
    if (!data) return;

    await withDocumentLifecycle('saving', 'Failed to export file', async () => {
      isLoading.value = true;
      if (!(await flushPendingCellChanges())) return;
      await documentSessionStore.waitForMutations();

      const isNewFile = isUntitledSpreadsheet(data.fileName);
      const defaultName = isNewFile ? 'untitled' : baseNameWithoutExtension(data.fileName);
      const capabilities = await documentCapabilities(
        data.fileName,
        documentSessionStore.currentFilePath
      );
      const extension = isNewFile
        ? await defaultSpreadsheetExtension()
        : capabilities.exportExtension;
      const exportedPath = await exportFile(`${defaultName}.${extension}`);
      if (exportedPath) {
        ElMessage.success('File exported successfully');
      }
    });
    isLoading.value = false;
  }

  async function handleBack() {
    if (!(await flushPendingCellChanges())) return;
    await documentSessionStore.waitForMutations();

    try {
      await api.closeCurrentDocument();
    } catch (error) {
      ElMessage.error(`Failed to close file: ${error}`);
      return;
    }
    documentSessionStore.clearDocument();
    resetDocumentStatus();
    router.push({ name: 'home' });
  }

  return {
    loadFileFromPath,
    handleOpenFile,
    handleSaveFile,
    handleExportFile,
    handleBack,
  };
}
