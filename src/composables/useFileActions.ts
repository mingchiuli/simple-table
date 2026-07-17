import type { ComputedRef } from 'vue';
import { ElMessage } from 'element-plus';

import * as api from '@/api';
import {
  discardOpenFileSelection,
  exportFile,
  pickOpenFile,
  prepareOpenFile,
  saveFile,
} from '@/platform';
import { createDocumentFileCoordinator } from '@/application/documentFileCoordinator';
import { useDocumentSessionCoordinator } from '@/composables/useDocumentSessionCoordinator';
import { createSpreadsheetFormatService } from '@/application/spreadsheetFormatService';
import { useDocumentCommandBus } from '@/composables/useDocumentCommandBus';
import { useDocumentLifecycle } from '@/composables/useDocumentLifecycle';
import { useDocumentReplacementGuard } from '@/composables/useDocumentReplacementGuard';
import { useRecentFileUpdates } from '@/composables/useRecentFileUpdates';
import { useSaveLocation } from '@/composables/useSaveLocation';
import { commitPreparedDocumentOrAbort } from '@/composables/preparedDocument';
import { useDocumentSessionStore } from '@/stores/documentSession';
import { useEditorSelectionStore } from '@/stores/editorSelection';
import type { DocumentProjection } from '@/types';
import { appErrorMessage } from '@/utils/appError';

type UseFileActionsOptions = {
  fileData: ComputedRef<DocumentProjection | null>;
  flushPendingCellChanges: () => Promise<boolean>;
};

export function useFileActions({
  fileData,
  flushPendingCellChanges,
}: UseFileActionsOptions) {
  const router = useRouter();
  const documentSessionStore = useDocumentSessionStore();
  const documentSessionCoordinator = useDocumentSessionCoordinator();
  const editorSelectionStore = useEditorSelectionStore();
  const { beginDocumentReplacement } = useDocumentReplacementGuard({
    flushPendingCellChanges,
  });
  const { withReservedSaveLocation } = useSaveLocation();
  const { runDocumentLifecycle } = useDocumentLifecycle();
  const commandBus = useDocumentCommandBus();
  const { queueRecentFileEntryUpdate } = useRecentFileUpdates();
  const spreadsheetFormats = createSpreadsheetFormatService(api);

  const fileCoordinator = createDocumentFileCoordinator({
    getFileData: () => fileData.value,
    getCommandContext: () => documentSessionStore.currentCommandContext(),
    getCurrentFilePath: () => documentSessionStore.currentFilePath,
    getCurrentSheetIndex: () => editorSelectionStore.currentSheetIndex,
    beginDocumentReplacement,
    runDocumentLifecycle,
    prepareConsistentContext: () =>
      commandBus.prepareConsistentContext(flushPendingCellChanges).then((context) => context ?? null),
    pickOpenFile,
    discardOpenFileSelection,
    prepareOpenFile,
    commitPreparedDocument: commitPreparedDocumentOrAbort,
    abortPreparedDocument: (prepared) => api.abortPreparedDocument(prepared.token),
    closeDocument: api.closeCurrentDocument,
    saveFile,
    exportFile,
    nativeSavePlan: api.getNativeSavePlan,
    documentCapabilities: api.getDocumentCapabilities,
    defaultSpreadsheetExtension: spreadsheetFormats.defaultSpreadsheetExtension,
    withReservedSaveLocation,
    openDocumentResponse: (response, path) =>
      documentSessionCoordinator.openDocumentResponse(response, path),
    applySavedDocumentResponse: (context, response, path, preferredSheetIndex) =>
      documentSessionCoordinator.applySavedDocumentResponseForContext(
        context,
        response,
        path,
        preferredSheetIndex
      ),
    clearDocument: () => documentSessionCoordinator.clearDocument(),
    queueRecentFileEntryUpdate,
    reportCleanupError: (message, error) => console.warn(`${message}:`, error),
  });

  async function handleOpenFile() {
    await fileCoordinator.openPickedFile();
  }

  async function handleSaveFile() {
    const outcome = await fileCoordinator.saveCurrentFile();
    if (outcome.status === 'saved') {
      ElMessage.success('File saved successfully');
    } else if (outcome.status === 'saved-stale') {
      ElMessage.warning(
        'File was saved, but the active document changed before the editor could refresh.'
      );
    } else if (outcome.status === 'blocked') {
      ElMessage.error(outcome.message);
    }
  }

  async function handleExportFile() {
    if (await fileCoordinator.exportCurrentFile() === 'exported') {
      ElMessage.success('File exported successfully');
    }
  }

  async function handleBack() {
    try {
      await router.push({ name: 'home' });
    } catch (error) {
      ElMessage.error(`Failed to return home: ${appErrorMessage(error)}`);
    }
  }

  return {
    loadFileFromPath: fileCoordinator.loadFileFromPath,
    handleOpenFile,
    handleSaveFile,
    handleExportFile,
    closeCurrentDocument: fileCoordinator.closeCurrentDocument,
    handleBack,
  };
}
