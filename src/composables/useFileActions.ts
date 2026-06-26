import type { ComputedRef, Ref } from 'vue';
import * as api from '@/api';
import { exportFile, getFileName, getStorageType, openFile, pickSaveLocation, readFile, saveFile } from '@/platform';
import { useFileDataStore } from '@/stores/fileData';
import { useRecentFilesStore } from '@/stores/recentFiles';
import type { FileData } from '@/types';

type UseFileActionsOptions = {
  fileData: ComputedRef<FileData | null>;
  currentSheetIndex: Ref<number>;
  isLoading: Ref<boolean>;
  isFileLoading: Ref<boolean>;
  flushPendingCellChanges: () => Promise<boolean>;
  refreshEditorState: () => Promise<void>;
  markSaved: () => Promise<void>;
  resetDocumentStatus: () => void;
};

function writableExtension(fileName: string): 'xlsx' | 'xlsm' | 'csv' | null {
  const extension = fileName.split('.').pop()?.toLowerCase() || 'xlsx';
  return extension === 'xlsx' || extension === 'xlsm' || extension === 'csv' ? extension : null;
}

export function useFileActions({
  fileData,
  currentSheetIndex,
  isLoading,
  isFileLoading,
  flushPendingCellChanges,
  refreshEditorState,
  markSaved,
  resetDocumentStatus,
}: UseFileActionsOptions) {
  const router = useRouter();
  const fileDataStore = useFileDataStore();
  const recentFilesStore = useRecentFilesStore();

  async function updateRecentFileEntry(
    path: string,
    fileName: string,
    storageType: 'mobileSandboxPath' | 'desktopPath',
    originalPath?: string
  ) {
    if (!fileData.value) return;

    const fileSize = await api.getFileSize(path);
    const bytes = await api.generateCurrentThumbnailBytes();
    await api.addRecentFileWithThumbnail(path, fileName, fileSize, bytes, storageType, originalPath);
  }

  async function loadFileFromPath(filePath: string) {
    try {
      isLoading.value = true;
      isFileLoading.value = true;
      const loadedFileData = await readFile(filePath);
      fileDataStore.set(loadedFileData, filePath);
      currentSheetIndex.value = 0;
      resetDocumentStatus();

      const fileName = await getFileName(filePath);
      const storageType = await getStorageType();
      await updateRecentFileEntry(filePath, fileName, storageType);
      await recentFilesStore.load();

      await refreshEditorState();
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

      const result = await openFile();
      if (!result) return;

      fileDataStore.set(result.fileData, result.path);
      currentSheetIndex.value = 0;
      resetDocumentStatus();

      const storageType = await getStorageType();
      const bytes = await api.generateCurrentThumbnailBytes();
      const fileSize = await api.getFileSize(result.path);
      await api.addRecentFileWithThumbnail(
        result.path,
        result.fileName,
        fileSize,
        bytes,
        storageType,
        result.originalPath
      );

      await refreshEditorState();
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

      const isNewFile = fileData.value.fileName.startsWith('untitled');
      const defaultName = isNewFile
        ? 'untitled'
        : fileData.value.fileName.replace(/\.[^.]+$/, '');

      const existingPath = fileDataStore.currentFilePath;
      const storageType = await getStorageType();
      const existingWritableExtension = existingPath ? writableExtension(existingPath) : null;

      if (existingPath && existingWritableExtension) {
        isLoading.value = true;
        await saveFile(existingPath);
        await markSaved();
        const fileName = await getFileName(existingPath);
        fileData.value.fileName = fileName;
        fileData.value.path = existingPath;
        await updateRecentFileEntry(existingPath, fileName, storageType);
        await recentFilesStore.load();
        ElMessage.success('File saved successfully');
        return;
      }

      const fallbackExtension = writableExtension(fileData.value.fileName) || 'xlsx';
      const savePath = await pickSaveLocation(`${defaultName}.${fallbackExtension}`);
      if (!savePath) return;

      if (!writableExtension(savePath)) {
        ElMessage.error('Saving is only supported as .xlsx, .xlsm, or .csv');
        return;
      }

      isLoading.value = true;
      await saveFile(savePath);
      await markSaved();

      const fileName = await getFileName(savePath);
      fileData.value.fileName = fileName;
      fileData.value.path = savePath;
      await updateRecentFileEntry(savePath, fileName, storageType);
      await recentFilesStore.load();
      fileDataStore.setPath(savePath);
      ElMessage.success('File saved successfully');
    } catch (error) {
      ElMessage.error(`Failed to save file: ${error}`);
    } finally {
      isLoading.value = false;
    }
  }

  async function ensureSandboxPathForExport(defaultName: string, extension: string): Promise<string | null> {
    if (!(await flushPendingCellChanges())) return null;
    if (!fileData.value) return null;

    let path = fileDataStore.currentFilePath;
    const storageType = await getStorageType();

    if (!path || !writableExtension(path)) {
      if (storageType === 'desktopPath') {
        throw new Error('Export is only supported for mobile sandbox files');
      }
      path = await pickSaveLocation(`${defaultName}.${extension}`);
      if (!path) return null;
      fileDataStore.setPath(path);

      const fileName = await getFileName(path);
      fileData.value.fileName = fileName;
      fileData.value.path = path;
    }

    await saveFile(path);
    await markSaved();
    const fileName = await getFileName(path);
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
      const extension = isNewFile ? 'xlsx' : writableExtension(fileData.value.fileName) || 'xlsx';
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

    fileDataStore.clear();
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
