import type { ComputedRef, Ref } from 'vue';
import * as api from '@/api';
import { exportFile, getFileName, getStorageType, openFile, pickSaveLocation, readFile, saveFile } from '@/platform';
import { useDocumentSessionStore } from '@/stores/documentSession';
import { useRecentFilesStore } from '@/stores/recentFiles';
import type { FileData } from '@/types';
import { documentCapabilities, exportExtensionFromName, nativeSaveExtensionFromName } from '@/utils/documentCapabilities';
import { waitForEditorMutations } from '@/composables/useEditorMutationQueue';

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

  async function syncSavedDocumentIdentity(path: string) {
    const fileName = await getFileName(path);
    await api.updateDocumentIdentity(path, fileName);
    documentSessionStore.updateIdentity(path, fileName);
    return fileName;
  }

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
      if (!(await flushPendingCellChanges())) return;
      await waitForEditorMutations();

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
      await waitForEditorMutations();

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
      await waitForEditorMutations();

      const isNewFile = fileData.value.fileName.startsWith('untitled');
      const defaultName = isNewFile
        ? 'untitled'
        : fileData.value.fileName.replace(/\.[^.]+$/, '');

      const existingPath = documentSessionStore.currentFilePath;
      const storageType = await getStorageType();
      const capabilities = await documentCapabilities(fileData.value.fileName, existingPath);

      if (existingPath && capabilities.nativeSaveExtension && !capabilities.requiresSaveAsForNativeSave) {
        isLoading.value = true;
        await saveFile(existingPath);
        const fileName = await syncSavedDocumentIdentity(existingPath);
        await markSaved();
        await updateRecentFileEntry(existingPath, fileName, storageType);
        await recentFilesStore.load();
        ElMessage.success('File saved successfully');
        return;
      }

      const fallbackExtension = capabilities.nativeSaveExtension || 'xlsx';
      const savePath = await pickSaveLocation(`${defaultName}.${fallbackExtension}`);
      if (!savePath) return;

      if (!nativeSaveExtensionFromName(savePath)) {
        ElMessage.error('Native save is only supported as .xlsx. Use export for CSV.');
        return;
      }

      isLoading.value = true;
      await saveFile(savePath);
      const fileName = await syncSavedDocumentIdentity(savePath);
      await markSaved();

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
    await waitForEditorMutations();
    if (!fileData.value) return null;

    let path = documentSessionStore.currentFilePath;
    const storageType = await getStorageType();

    if (!path || !exportExtensionFromName(path)) {
      if (storageType === 'desktopPath') {
        throw new Error('Export is only supported for mobile sandbox files');
      }
      path = await pickSaveLocation(`${defaultName}.${extension}`);
      if (!path) return null;
    }

    await saveFile(path);
    const fileName = await syncSavedDocumentIdentity(path);
    await markSaved();
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
        await waitForEditorMutations();
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
    await waitForEditorMutations();

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
