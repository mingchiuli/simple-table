import type { ComputedRef } from 'vue';

import * as api from '@/api';
import { createDocumentFileCoordinator } from '@/application/documentFileCoordinator';
import { createSpreadsheetFormatService } from '@/application/spreadsheetFormatService';
import {
  runtimeDocumentCapabilities,
  runtimeNativeSavePlan,
  runtimeSpreadsheetFormatOptions,
} from '@/application/fileProtocol';
import { useDocumentCommandBus } from '@/composables/useDocumentCommandBus';
import { useDocumentLifecycle } from '@/composables/useDocumentLifecycle';
import { useDocumentReplacementGuard } from '@/composables/useDocumentReplacementGuard';
import { useDocumentSessionCoordinator } from '@/composables/useDocumentSessionCoordinator';
import { useRecentFileUpdates } from '@/composables/useRecentFileUpdates';
import { useSaveLocation } from '@/composables/useSaveLocation';
import {
  discardOpenFileSelection,
  exportFile,
  pickOpenFile,
  prepareOpenFile,
  prepareRecentFile,
  saveFile,
} from '@/platform';
import { useDocumentSessionStore } from '@/stores/documentSession';
import { useEditorSelectionStore } from '@/stores/editorSelection';
import type { DocumentProjection } from '@/types/documentRuntime';

type UseDocumentFileCoordinatorOptions = {
  fileData?: ComputedRef<DocumentProjection | null>;
  flushPendingCellChanges?: () => Promise<boolean>;
};

export function useDocumentFileCoordinator({
  fileData,
  flushPendingCellChanges,
}: UseDocumentFileCoordinatorOptions = {}) {
  const document = useDocumentSessionStore();
  const session = useDocumentSessionCoordinator();
  const selection = useEditorSelectionStore();
  const commandBus = useDocumentCommandBus();
  const { beginDocumentReplacement } = useDocumentReplacementGuard({
    flushPendingCellChanges,
  });
  const { runDocumentLifecycle } = useDocumentLifecycle();
  const { withReservedSaveLocation } = useSaveLocation();
  const { queueRecentFileEntryUpdate } = useRecentFileUpdates();
  const spreadsheetFormats = createSpreadsheetFormatService({
    getSpreadsheetFormatOptions: async () =>
      runtimeSpreadsheetFormatOptions(await api.getSpreadsheetFormatOptions()),
  });

  return createDocumentFileCoordinator({
    getFileData: () => fileData?.value ?? document.data,
    getCommandContext: () => document.currentCommandContext(),
    getCurrentFilePath: () => document.currentFilePath,
    getCurrentSheetIndex: () => selection.currentSheetIndex,
    beginDocumentReplacement,
    runDocumentLifecycle,
    prepareConsistentContext: () => flushPendingCellChanges
      ? commandBus.prepareConsistentContext(flushPendingCellChanges).then((context) => context ?? null)
      : Promise.resolve(document.currentCommandContext()),
    pickOpenFile: () => pickOpenFile(),
    discardOpenFileSelection: (selection) => discardOpenFileSelection(selection),
    prepareOpenFile: (path) => prepareOpenFile(path),
    prepareRecentFile: (file) => prepareRecentFile(file),
    prepareNewFile: () => api.prepareNewFile(),
    commitPreparedDocument: (prepared, expectedContext) =>
      api.commitPreparedDocument(prepared.token, expectedContext),
    openedDocumentId: (opened) => opened.editorSession.documentId,
    abortPreparedDocument: (prepared) => api.abortPreparedDocument(prepared.token),
    closeDocument: (documentId) => api.closeCurrentDocument(documentId),
    saveFile: (path, context) => saveFile(path, context),
    exportFile: (defaultName, context) => exportFile(defaultName, context),
    nativeSavePlan: async (context, target) =>
      runtimeNativeSavePlan(await api.getNativeSavePlan(context, target)),
    documentCapabilities: async (context) =>
      runtimeDocumentCapabilities(await api.getDocumentCapabilities(context)),
    defaultSpreadsheetExtension: spreadsheetFormats.defaultSpreadsheetExtension,
    withReservedSaveLocation,
    openDocumentResponse: (response, path) => session.openDocumentResponse(response, path),
    applySavedDocumentResponse: (context, response, path, preferredSheetIndex) =>
      session.applySavedDocumentResponseForContext(
        context,
        response,
        path,
        preferredSheetIndex,
      ),
    clearDocument: () => session.clearDocument(),
    queueRecentFileEntryUpdate,
    reportCleanupError: (message, error) => console.warn(`${message}:`, error),
  });
}
