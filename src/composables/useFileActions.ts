import type { ComputedRef, Ref } from 'vue';
import * as api from '@/api';
import { exportFile, getFileName, getStorageType, openFile, pickSaveLocation, readFile, saveFile } from '@/platform';
import { useDocumentSessionStore } from '@/stores/documentSession';
import { useRecentFilesStore } from '@/stores/recentFiles';
import type { FileData } from '@/types';
import { documentCapabilities, exportExtension, nativeSaveExtension } from '@/utils/documentCapabilities';

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
    const bytes = await api.generateCurrentThumbnailBytes();
    await api.addRecentFileWithThumbnail(path, fileName, fileSize, bytes, storageType, originalPath);
  }

  async function loadFileFromPath(filePath: string) {
    try {
      isLoading.value = true;
      isFileLoading.value = true;
      const loadedFileData = await readFile(filePath);
      documentSessionStore.openDocument(loadedFileData, filePath);
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

      documentSessionStore.openDocument(result.fileData, result.path);
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

      const existingPath = documentSessionStore.currentFilePath;
      const storageType = await getStorageType();
      const capabilities = documentCapabilities(fileData.value.fileName, existingPath);

      if (existingPath && capabilities.nativeSaveExtension && !capabilities.requiresSaveAsForNativeSave) {
        isLoading.value = true;
        await saveFile(existingPath);
        await markSaved();
        const fileName = await getFileName(existingPath);
        documentSessionStore.updateIdentity(existingPath, fileName);
        await updateRecentFileEntry(existingPath, fileName, storageType);
        await recentFilesStore.load();
        ElMessage.success('File saved successfully');
        return;
      }

      const fallbackExtension = capabilities.nativeSaveExtension || 'xlsx';
      const savePath = await pickSaveLocation(`${defaultName}.${fallbackExtension}`);
      if (!savePath) return;

      if (!nativeSaveExtension(savePath)) {
        ElMessage.error('Native save is only supported as .xlsx. Use export for CSV.');
        return;
      }

      isLoading.value = true;
      await saveFile(savePath);
      await markSaved();

      const fileName = await getFileName(savePath);
      documentSessionStore.updateIdentity(savePath, fileName);
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
    if (!fileData.value) return null;

    let path = documentSessionStore.currentFilePath;
    const storageType = await getStorageType();

    if (!path || !exportExtension(path)) {
      if (storageType === 'desktopPath') {
        throw new Error('Export is only supported for mobile sandbox files');
      }
      path = await pickSaveLocation(`${defaultName}.${extension}`);
      if (!path) return null;

      const fileName = await getFileName(path);
      documentSessionStore.updateIdentity(path, fileName);
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
      const capabilities = documentCapabilities(fileData.value.fileName, documentSessionStore.currentFilePath);
      const extension = isNewFile ? 'xlsx' : capabilities.exportExtension;
      const storageType = await getStorageType();

      if (storageType === 'desktopPath') {
        if (!(await flushPendingCellChanges())) return;
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
