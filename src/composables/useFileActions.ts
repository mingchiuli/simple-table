import type { ComputedRef, Ref } from 'vue';
import { ElMessage } from 'element-plus';
import * as api from '@/api';
import {
  exportFile,
  getFileName,
  pickOpenFile,
  readFile,
  saveFile,
} from '@/platform';
import { useDocumentReplacementGuard } from '@/composables/useDocumentReplacementGuard';
import { useDocumentLifecycle } from '@/composables/useDocumentLifecycle';
import { useOpenFileSelection } from '@/composables/useOpenFileSelection';
import { useRecentFileUpdates } from '@/composables/useRecentFileUpdates';
import { useSaveLocation } from '@/composables/useSaveLocation';
import { useDocumentSessionStore } from '@/stores/documentSession';
import type { FileData } from '@/types';
import { documentCapabilities, nativeSavePlan } from '@/utils/documentCapabilities';
import { baseNameWithoutExtension, isUntitledSpreadsheet } from '@/utils/fileFormats';
import { defaultSpreadsheetExtension } from '@/utils/spreadsheetFormats';

type UseFileActionsOptions = {
  fileData: ComputedRef<FileData | null>;
  currentSheetIndex: Ref<number>;
  isLoading: Ref<boolean>;
  isFileLoading: Ref<boolean>;
  flushPendingCellChanges: () => Promise<boolean>;
};

type ContinuationGuard = (() => boolean) & {
  onCancel?: (handler: () => void) => () => void;
};

function keepGoing() {
  return true;
}

export function useFileActions({
  fileData,
  currentSheetIndex,
  isLoading,
  isFileLoading,
  flushPendingCellChanges,
}: UseFileActionsOptions) {
  const router = useRouter();
  const documentSessionStore = useDocumentSessionStore();
  const { prepareForDocumentReplacement } = useDocumentReplacementGuard({
    flushPendingCellChanges,
  });
  const { openSelectedFileOrDiscard } = useOpenFileSelection({
    prepareForDocumentReplacement,
  });
  const { withReservedSaveLocation } = useSaveLocation();
  const { runDocumentLifecycle } = useDocumentLifecycle();
  const { queueRecentFileEntryUpdate } = useRecentFileUpdates();

  async function loadFileFromPath(
    filePath: string,
    shouldContinue: ContinuationGuard = keepGoing
  ): Promise<boolean> {
    let loaded = false;
    let releasedByCancel = false;
    let removeCancelHandler: (() => void) | undefined;
    const lifecycleStatus = await runDocumentLifecycle(
      'loading',
      'Failed to open file',
      async () => {
        isLoading.value = true;
        isFileLoading.value = true;
        removeCancelHandler = shouldContinue.onCancel?.(() => {
          releasedByCancel = true;
          isLoading.value = false;
          isFileLoading.value = false;
          documentSessionStore.endLifecycle('loading');
        });
        if (!shouldContinue()) return;
        if (!(await prepareForDocumentReplacement())) return;
        if (!shouldContinue()) return;

        const opened = await readFile(filePath);
        if (!shouldContinue()) return;
        const fileName = opened.fileData.fileName || await getFileName(filePath);
        if (!shouldContinue()) return;

        documentSessionStore.openDocumentResponse(opened, filePath);
        currentSheetIndex.value = 0;

        queueRecentFileEntryUpdate(filePath, fileName);
        loaded = true;
      },
      { waitForIdle: true, shouldContinue }
    );
    removeCancelHandler?.();
    if (lifecycleStatus !== 'skipped' && !releasedByCancel) {
      isLoading.value = false;
      isFileLoading.value = false;
    }
    return loaded;
  }

  async function handleOpenFile() {
    const lifecycleStatus = await runDocumentLifecycle('loading', 'Failed to open file', async () => {
      isLoading.value = true;
      isFileLoading.value = true;

      const selection = await pickOpenFile();
      if (!selection) return;
      const opened = await openSelectedFileOrDiscard(selection);
      if (!opened) return;
      documentSessionStore.openDocumentResponse(opened, selection.path);
      currentSheetIndex.value = 0;

      queueRecentFileEntryUpdate(selection.path, selection.fileName, selection.originalPath);
    });
    if (lifecycleStatus !== 'skipped') {
      isLoading.value = false;
      isFileLoading.value = false;
    }
  }

  async function handleSaveFile() {
    const lifecycleStatus = await runDocumentLifecycle('saving', 'Failed to save file', async () => {
      const data = fileData.value;
      if (!data) return;
      if (!(await flushPendingCellChanges())) return;
      await documentSessionStore.waitForMutations();
      const context = documentSessionStore.requireCommandContext();

      const isNewFile = isUntitledSpreadsheet(data.fileName);
      const defaultName = isNewFile ? 'untitled' : baseNameWithoutExtension(data.fileName);

      const existingPath = documentSessionStore.currentFilePath;
      const existingTarget = existingPath ?? data.fileName;
      const savePlan = await nativeSavePlan(context, existingTarget);

      if (existingPath && savePlan.canSave && !savePlan.requiresSaveAs) {
        isLoading.value = true;
        const saved = await saveFile(existingPath, context);
        if (!documentSessionStore.applySavedDocumentResponseForContext(context, saved, existingPath)) {
          notifySavedButNotApplied();
          return;
        }
        const fileName = saved.fileData.fileName || await getFileName(existingPath);
        queueRecentFileEntryUpdate(existingPath, fileName);
        ElMessage.success('File saved successfully');
        return;
      }

      if (existingPath && !savePlan.requiresSaveAs && !savePlan.canSave) {
        ElMessage.error(savePlan.blockedReason ?? 'Workbook cannot be saved in its current state.');
        return;
      }

      const fallbackExtension = savePlan.defaultExtension;
      await withReservedSaveLocation(`${defaultName}.${fallbackExtension}`, async ({
        path: savePath,
        markPersisted,
      }) => {
        const targetPlan = await nativeSavePlan(context, savePath);
        if (!targetPlan.canSave) {
          ElMessage.error(targetPlan.blockedReason ?? 'Workbook cannot be saved in its current state.');
          return;
        }

        isLoading.value = true;
        const saved = await saveFile(savePath, context);
        markPersisted();
        if (!documentSessionStore.applySavedDocumentResponseForContext(context, saved, savePath)) {
          notifySavedButNotApplied();
          return;
        }
        const fileName = saved.fileData.fileName || await getFileName(savePath);

        queueRecentFileEntryUpdate(savePath, fileName);
        ElMessage.success('File saved successfully');
      });
    });
    if (lifecycleStatus !== 'skipped') {
      isLoading.value = false;
    }
  }

  async function handleExportFile() {
    const lifecycleStatus = await runDocumentLifecycle('saving', 'Failed to export file', async () => {
      const data = fileData.value;
      if (!data) return;
      isLoading.value = true;
      if (!(await flushPendingCellChanges())) return;
      await documentSessionStore.waitForMutations();
      const context = documentSessionStore.requireCommandContext();

      const isNewFile = isUntitledSpreadsheet(data.fileName);
      const defaultName = isNewFile ? 'untitled' : baseNameWithoutExtension(data.fileName);
      const capabilities = await documentCapabilities(
        context,
        data.fileName,
        documentSessionStore.currentFilePath
      );
      const extension = isNewFile
        ? await defaultSpreadsheetExtension()
        : capabilities.exportExtension;
      const exportedPath = await exportFile(`${defaultName}.${extension}`, context);
      if (exportedPath) {
        ElMessage.success('File exported successfully');
      }
    });
    if (lifecycleStatus !== 'skipped') {
      isLoading.value = false;
    }
  }

  async function closeCurrentDocument(): Promise<boolean> {
    if (documentSessionStore.isInteractionLocked) return false;
    if (!(await prepareForDocumentReplacement())) return false;

    const context = documentSessionStore.currentCommandContext();
    if (!context) {
      documentSessionStore.clearDocument();
      return true;
    }

    try {
      await api.closeCurrentDocument(context.documentId);
    } catch (error) {
      ElMessage.error(`Failed to close file: ${error}`);
      return false;
    }
    documentSessionStore.clearDocument();
    return true;
  }

  async function handleBack() {
    if (!(await closeCurrentDocument())) return;
    router.push({ name: 'home' });
  }

  function notifySavedButNotApplied() {
    ElMessage.warning(
      'File was saved, but the active document changed before the editor could refresh.'
    );
  }

  return {
    loadFileFromPath,
    handleOpenFile,
    handleSaveFile,
    handleExportFile,
    closeCurrentDocument,
    handleBack,
  };
}
