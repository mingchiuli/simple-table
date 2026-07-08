import type { ComputedRef, Ref } from 'vue';
import * as api from '@/api';
import { exportFile, getFileName, getStorageType, openFile, pickSaveLocation, readFile, saveFile } from '@/platform';
import { useDocumentSessionStore } from '@/stores/documentSession';
import { useRecentFilesStore } from '@/stores/recentFiles';
import type { FileData } from '@/types';
import { documentCapabilities, nativeSavePlan } from '@/utils/documentCapabilities';

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
    storageType: 'mobileSandboxPath' | 'desktopPath',
    originalPath?: string
  ) {
    if (!fileData.value) return;

    const fileSize = await api.getFileSize(path);
    await api.addRecentFileWithThumbnail(path, fileName, fileSize, storageType, originalPath);
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
      const storageType = await getStorageType();
      await updateRecentFileEntry(filePath, fileName, storageType);
      await recentFilesStore.load();
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

      const storageType = await getStorageType();
      const fileSize = await api.getFileSize(result.path);
      await api.addRecentFileWithThumbnail(
        result.path,
        result.fileName,
        fileSize,
        storageType,
        result.originalPath
      );
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

      const isNewFile = data.fileName.startsWith('untitled');
      const defaultName = isNewFile
        ? 'untitled'
        : data.fileName.replace(/\.[^.]+$/, '');

      const existingPath = documentSessionStore.currentFilePath;
      const storageType = await getStorageType();
      const existingTarget = existingPath ?? data.fileName;
      const savePlan = await nativeSavePlan(existingTarget);

      if (existingPath && savePlan.canSave && !savePlan.requiresSaveAs) {
        isLoading.value = true;
        const saved = await saveFile(existingPath);
        documentSessionStore.applySavedDocumentResponse(saved, existingPath);
        const fileName = saved.fileData.fileName || await getFileName(existingPath);
        await updateRecentFileEntry(existingPath, fileName, storageType);
        await recentFilesStore.load();
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

      await updateRecentFileEntry(savePath, fileName, storageType);
      await recentFilesStore.load();
      ElMessage.success('File saved successfully');
    });
    isLoading.value = false;
  }

  async function ensureSandboxPathForExport(defaultName: string, extension: string): Promise<string | null> {
    if (!(await flushPendingCellChanges())) return null;
    await documentSessionStore.waitForMutations();
    if (!fileData.value) return null;

    let path = documentSessionStore.currentFilePath;
    const storageType = await getStorageType();
    const pathPlan = path ? await nativeSavePlan(path) : null;

    if (!path || pathPlan?.requiresSaveAs) {
      if (storageType === 'desktopPath') {
        throw new Error('Export is only supported for mobile sandbox files');
      }
      path = await pickSaveLocation(`${defaultName}.${extension}`);
      if (!path) return null;

      const targetPlan = await nativeSavePlan(path);
      if (!targetPlan.canSave) {
        throw new Error(targetPlan.blockedReason ?? 'Workbook cannot be saved in its current state.');
      }
    } else if (pathPlan && !pathPlan.canSave) {
      throw new Error(pathPlan.blockedReason ?? 'Workbook cannot be saved in its current state.');
    }

    const saved = await saveFile(path);
    documentSessionStore.applySavedDocumentResponse(saved, path);
    const fileName = saved.fileData.fileName || await getFileName(path);
    await updateRecentFileEntry(path, fileName, storageType);
    await recentFilesStore.load();
    return path;
  }

  async function handleExportFile() {
    const data = fileData.value;
    if (!data) return;

    await withDocumentLifecycle('saving', 'Failed to export file', async () => {
      isLoading.value = true;
      const isNewFile = data.fileName.startsWith('untitled');
      const defaultName = isNewFile
        ? 'untitled'
        : data.fileName.replace(/\.[^.]+$/, '');
      const capabilities = await documentCapabilities(
        data.fileName,
        documentSessionStore.currentFilePath
      );
      const extension = isNewFile ? 'xlsx' : capabilities.exportExtension;
      const storageType = await getStorageType();

      if (storageType === 'desktopPath') {
        if (!(await flushPendingCellChanges())) return;
        await documentSessionStore.waitForMutations();
        const exportedPath = await exportFile(
          documentSessionStore.currentFilePath ?? '',
          `${defaultName}.${extension}`
        );
        if (exportedPath) {
          ElMessage.success('File exported successfully');
        }
        return;
      }

      const sourcePath = await ensureSandboxPathForExport(defaultName, extension);
      if (!sourcePath) return;

      const exportedPath = await exportFile(sourcePath, `${defaultName}.${extension}`);
      if (exportedPath) {
        ElMessage.success('File exported successfully');
      }
    });
    isLoading.value = false;
  }

  async function handleBack() {
    if (!(await flushPendingCellChanges())) return;
    await documentSessionStore.waitForMutations();

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
