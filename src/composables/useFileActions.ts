import type { ComputedRef } from 'vue';
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
import { useEditorSelectionStore } from '@/stores/editorSelection';
import type { FileData } from '@/types';
import { documentCapabilities, nativeSavePlan } from '@/utils/documentCapabilities';
import { baseNameWithoutExtension, isUntitledSpreadsheet } from '@/utils/fileFormats';
import { warnRecentFileTrackingFailure } from '@/utils/recentFileTracking';
import { defaultSpreadsheetExtension } from '@/utils/spreadsheetFormats';

type UseFileActionsOptions = {
  fileData: ComputedRef<FileData | null>;
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
  isLoading,
  isFileLoading,
  flushPendingCellChanges,
}: UseFileActionsOptions) {
  const router = useRouter();
  const documentSessionStore = useDocumentSessionStore();
  const editorSelectionStore = useEditorSelectionStore();
  const { beginDocumentReplacement } = useDocumentReplacementGuard({
    flushPendingCellChanges,
  });
  const { openSelectedFileOrDiscard } = useOpenFileSelection({
    beginDocumentReplacement,
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
      async ({ release }) => {
        isLoading.value = true;
        isFileLoading.value = true;
        removeCancelHandler = shouldContinue.onCancel?.(() => {
          releasedByCancel = true;
          isLoading.value = false;
          isFileLoading.value = false;
          release();
        });
        if (!shouldContinue()) return;
        const replacement = await beginDocumentReplacement();
        if (!replacement) return;

        try {
          if (!shouldContinue()) return;
          const opened = await awaitRouteLoadStep(readFile(filePath), shouldContinue);
          if (!opened) return;
          replacement.commit();
          documentSessionStore.openDocumentResponse(opened, filePath);
          editorSelectionStore.activateSheet(0);
          loaded = true;

          const fileName = await resolveRecentFileNameAfterOpen(
            opened.fileData.fileName,
            filePath,
            shouldContinue
          );
          if (!shouldContinue()) return;

          if (fileName) {
            queueRecentFileEntryUpdate(filePath, fileName);
          }
        } finally {
          if (!loaded) {
            replacement.cancel();
          }
        }
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
        const fileName = await resolveRecentFileNameAfterSave(saved.fileData.fileName, existingPath);
        if (fileName) {
          queueRecentFileEntryUpdate(existingPath, fileName);
        }
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
        const fileName = await resolveRecentFileNameAfterSave(saved.fileData.fileName, savePath);
        if (fileName) {
          queueRecentFileEntryUpdate(savePath, fileName);
        }
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
    const replacement = await beginDocumentReplacement();
    if (!replacement) return false;

    const context = documentSessionStore.currentCommandContext();
    if (!context) {
      replacement.commit();
      documentSessionStore.clearDocument();
      return true;
    }

    try {
      await api.closeCurrentDocument(context.documentId);
    } catch (error) {
      replacement.cancel();
      ElMessage.error(`Failed to close file: ${error}`);
      return false;
    }
    replacement.commit();
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

async function awaitRouteLoadStep<T>(
  promise: Promise<T>,
  shouldContinue: ContinuationGuard
): Promise<T | undefined> {
  try {
    const result = await promise;
    if (!shouldContinue()) {
      return undefined;
    }
    return result;
  } catch (error) {
    if (!shouldContinue()) {
      return undefined;
    }
    throw error;
  }
}

async function resolveRecentFileNameAfterOpen(
  openedFileName: string,
  filePath: string,
  shouldContinue: ContinuationGuard
): Promise<string | null> {
  if (openedFileName) return openedFileName;
  try {
    return await awaitRouteLoadStep(getFileName(filePath), shouldContinue) ?? null;
  } catch (error) {
    warnRecentFileTrackingFailure(error);
    return null;
  }
}

async function resolveRecentFileNameAfterSave(
  savedFileName: string,
  filePath: string
): Promise<string | null> {
  if (savedFileName) return savedFileName;
  try {
    return await getFileName(filePath);
  } catch (error) {
    warnRecentFileTrackingFailure(error);
    return null;
  }
}
