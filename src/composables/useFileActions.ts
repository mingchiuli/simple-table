import type { ComputedRef } from 'vue';
import { ElMessage } from 'element-plus';
import * as api from '@/api';
import {
  exportFile,
  pickOpenFile,
  prepareOpenFile,
  saveFile,
} from '@/platform';
import { useDocumentReplacementGuard } from '@/composables/useDocumentReplacementGuard';
import { useDocumentLifecycle } from '@/composables/useDocumentLifecycle';
import { useDocumentCommandBus } from '@/composables/useDocumentCommandBus';
import { useOpenFileSelection } from '@/composables/useOpenFileSelection';
import { useRecentFileUpdates } from '@/composables/useRecentFileUpdates';
import { useSaveLocation } from '@/composables/useSaveLocation';
import { commitPreparedDocumentOrAbort } from '@/composables/preparedDocument';
import { useDocumentSessionStore } from '@/stores/documentSession';
import type { DocumentProjection } from '@/types';
import { documentCapabilities, nativeSavePlan } from '@/utils/documentCapabilities';
import { baseNameWithoutExtension, isUntitledSpreadsheet } from '@/utils/fileFormats';
import { defaultSpreadsheetExtension } from '@/utils/spreadsheetFormats';
import { appErrorMessage } from '@/utils/appError';

type UseFileActionsOptions = {
  fileData: ComputedRef<DocumentProjection | null>;
  flushPendingCellChanges: () => Promise<boolean>;
};

type CloseCurrentDocumentOptions = {
  waitForIdle?: boolean;
};

type ContinuationGuard = (() => boolean) & {
  onCancel?: (handler: () => void) => () => void;
};

function keepGoing() {
  return true;
}

export function useFileActions({
  fileData,
  flushPendingCellChanges,
}: UseFileActionsOptions) {
  const router = useRouter();
  const documentSessionStore = useDocumentSessionStore();
  const { beginDocumentReplacement } = useDocumentReplacementGuard({
    flushPendingCellChanges,
  });
  const { openSelectedFileOrDiscard } = useOpenFileSelection({
    beginDocumentReplacement,
  });
  const { withReservedSaveLocation } = useSaveLocation();
  const { runDocumentLifecycle } = useDocumentLifecycle();
  const commandBus = useDocumentCommandBus();
  const { queueRecentFileEntryUpdate } = useRecentFileUpdates();

  async function loadFileFromPath(
    filePath: string,
    shouldContinue: ContinuationGuard = keepGoing
  ): Promise<boolean> {
    let loaded = false;
    let removeCancelHandler: (() => void) | undefined;
    await runDocumentLifecycle(
      'loading',
      'Failed to open file',
      async ({ release }) => {
        removeCancelHandler = shouldContinue.onCancel?.(() => {
          release();
        });
        if (!shouldContinue()) return;
        const replacement = await beginDocumentReplacement();
        if (!replacement) return;
        try {
          if (!shouldContinue()) return;
          const expectedContext = documentSessionStore.currentCommandContext();
          const preparedResult = await awaitRouteLoadStep(
            prepareOpenFile(filePath),
            shouldContinue,
            abortPreparedDocumentQuietly
          );
          if (!preparedResult) return;
          removeCancelHandler?.();
          removeCancelHandler = undefined;
          const opened = await commitPreparedDocumentOrAbort(preparedResult, expectedContext);
          if (!shouldContinue()) {
            try {
              await api.closeCurrentDocument(opened.editorSession.documentId);
              replacement.commit();
              documentSessionStore.clearDocument();
            } catch (error) {
              replacement.commit();
              documentSessionStore.openDocumentResponse(opened, filePath);
              throw error;
            }
            return;
          }
          replacement.commit();
          documentSessionStore.openDocumentResponse(opened, filePath);
          loaded = true;

          queueRecentFileEntryUpdate();
        } finally {
          if (!loaded) {
            replacement.cancel();
          }
        }
      },
      { waitForIdle: true, shouldContinue }
    );
    removeCancelHandler?.();
    return loaded;
  }

  async function handleOpenFile() {
    await runDocumentLifecycle('loading', 'Failed to open file', async () => {
      const selection = await pickOpenFile();
      if (!selection) return;
      const opened = await openSelectedFileOrDiscard(selection);
      if (!opened) return;

      queueRecentFileEntryUpdate(selection.originalPath);
    });
  }

  async function handleSaveFile() {
    await runDocumentLifecycle('saving', 'Failed to save file', async () => {
      const data = fileData.value;
      if (!data) return;
      const context = await commandBus.prepareConsistentContext(flushPendingCellChanges);
      if (!context) return;

      const isNewFile = isUntitledSpreadsheet(data.fileName);
      const defaultName = isNewFile ? 'untitled' : baseNameWithoutExtension(data.fileName);

      const existingPath = documentSessionStore.currentFilePath;
      const existingTarget = existingPath ?? data.fileName;
      const savePlan = await nativeSavePlan(context, existingTarget);

      if (existingPath && savePlan.canSave && !savePlan.requiresSaveAs) {
        const saved = await saveFile(existingPath, context);
        if (!documentSessionStore.applySavedDocumentResponseForContext(context, saved, existingPath)) {
          notifySavedButNotApplied();
          return;
        }
        queueRecentFileEntryUpdate();
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

        const saved = await saveFile(savePath, context);
        markPersisted();
        if (!documentSessionStore.applySavedDocumentResponseForContext(context, saved, savePath)) {
          notifySavedButNotApplied();
          return;
        }
        queueRecentFileEntryUpdate();
        ElMessage.success('File saved successfully');
      });
    });
  }

  async function handleExportFile() {
    await runDocumentLifecycle('saving', 'Failed to export file', async () => {
      const data = fileData.value;
      if (!data) return;
      const context = await commandBus.prepareConsistentContext(flushPendingCellChanges);
      if (!context) return;

      const isNewFile = isUntitledSpreadsheet(data.fileName);
      const defaultName = isNewFile ? 'untitled' : baseNameWithoutExtension(data.fileName);
      const capabilities = await documentCapabilities(context);
      const extension = isNewFile
        ? await defaultSpreadsheetExtension()
        : capabilities.exportExtension;
      const exportedPath = await exportFile(`${defaultName}.${extension}`, context);
      if (exportedPath) {
        ElMessage.success('File exported successfully');
      }
    });
  }

  async function closeCurrentDocument(
    options: CloseCurrentDocumentOptions = {}
  ): Promise<boolean> {
    let closed = false;
    const lifecycleStatus = await runDocumentLifecycle('closing', 'Failed to close file', async () => {
      const replacement = await beginDocumentReplacement();
      if (!replacement) return;

      const context = documentSessionStore.currentCommandContext();
      if (!context) {
        replacement.commit();
        documentSessionStore.clearDocument();
        closed = true;
        return;
      }

      try {
        await api.closeCurrentDocument(context.documentId);
      } catch (error) {
        replacement.cancel();
        throw error;
      }
      replacement.commit();
      documentSessionStore.clearDocument();
      closed = true;
    }, {
      waitForIdle: options.waitForIdle,
    });
    return lifecycleStatus !== 'skipped' && closed;
  }

  async function handleBack() {
    try {
      await router.push({ name: 'home' });
    } catch (error) {
      ElMessage.error(`Failed to return home: ${appErrorMessage(error)}`);
    }
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

async function abortPreparedDocumentQuietly(prepared: { token: string }) {
  try {
    await api.abortPreparedDocument(prepared.token);
  } catch (error) {
    console.warn('Failed to abort unused prepared document:', error);
  }
}

async function awaitRouteLoadStep<T>(
  promise: Promise<T>,
  shouldContinue: ContinuationGuard,
  discardResult: (result: T) => Promise<void>
): Promise<T | undefined> {
  try {
    const result = await promise;
    if (!shouldContinue()) {
      await discardResult(result);
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
