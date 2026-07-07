import type { ComputedRef, Ref } from 'vue';
import * as api from '@/api';
import { exportFile, getFileName, getStorageType, openFile, pickSaveLocation, readFile, saveFile } from '@/platform';
import { useDocumentSessionStore } from '@/stores/documentSession';
import { useRecentFilesStore } from '@/stores/recentFiles';
import type { FileData } from '@/types';
import { documentCapabilities, nativeSavePlan } from '@/utils/documentCapabilities';
import { waitForEditorMutations } from '@/composables/useEditorMutationQueue';

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
    try {
      isLoading.value = true;
      isFileLoading.value = true;
      if (!(await flushPendingCellChanges())) return;
      await waitForEditorMutations(documentSessionStore.mutationScope);

      const opened = await readFile(filePath);
      documentSessionStore.openDocumentResponse(opened, filePath);
      currentSheetIndex.value = 0;

      const fileName = await getFileName(filePath);
      const storageType = await getStorageType();
      await updateRecentFileEntry(filePath, fileName, storageType);
      await recentFilesStore.load();
    } catch (error) {
      ElMessage.error(`Failed to open file: ${error}`);
    } finally {
      isLoading.value = false;
      isFileLoading.value = false;
    }
  }

  async function handleOpenFile() {
    try {
      isLoading.value = true;
      isFileLoading.value = true;
      if (!(await flushPendingCellChanges())) return;
      await waitForEditorMutations(documentSessionStore.mutationScope);

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
    } catch (error) {
      ElMessage.error(`Failed to open file: ${error}`);
    } finally {
      isLoading.value = false;
      isFileLoading.value = false;
    }
  }

  async function handleSaveFile() {
    if (!fileData.value) return;

    try {
      if (!(await flushPendingCellChanges())) return;
      await waitForEditorMutations(documentSessionStore.mutationScope);

      const isNewFile = fileData.value.fileName.startsWith('untitled');
      const defaultName = isNewFile
        ? 'untitled'
        : fileData.value.fileName.replace(/\.[^.]+$/, '');

      const existingPath = documentSessionStore.currentFilePath;
      const storageType = await getStorageType();
      const existingTarget = existingPath ?? fileData.value.fileName;
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
    } catch (error) {
      ElMessage.error(`Failed to save file: ${error}`);
    } finally {
      isLoading.value = false;
    }
  }

  async function ensureSandboxPathForExport(defaultName: string, extension: string): Promise<string | null> {
    if (!(await flushPendingCellChanges())) return null;
    await waitForEditorMutations(documentSessionStore.mutationScope);
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
    if (!fileData.value) return;

    try {
      isLoading.value = true;
      const isNewFile = fileData.value.fileName.startsWith('untitled');
      const defaultName = isNewFile
        ? 'untitled'
        : fileData.value.fileName.replace(/\.[^.]+$/, '');
      const capabilities = await documentCapabilities(
        fileData.value.fileName,
        documentSessionStore.currentFilePath
      );
      const extension = isNewFile ? 'xlsx' : capabilities.exportExtension;
      const storageType = await getStorageType();

      if (storageType === 'desktopPath') {
        if (!(await flushPendingCellChanges())) return;
        await waitForEditorMutations(documentSessionStore.mutationScope);
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
    } catch (error) {
      ElMessage.error(`Failed to export file: ${error}`);
    } finally {
      isLoading.value = false;
    }
  }

  async function handleBack() {
    if (!(await flushPendingCellChanges())) return;
    await waitForEditorMutations(documentSessionStore.mutationScope);

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
