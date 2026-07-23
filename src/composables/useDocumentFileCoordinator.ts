import type { ComputedRef } from 'vue';

import * as api from '@/api';
import { createDocumentFileCoordinator } from '@/application/documentFileCoordinator';
import type { DocumentPreparationCoordinator } from '@/application/documentPreparationCoordinator';
import { createSpreadsheetFormatService } from '@/application/spreadsheetFormatService';
import {
  runtimeDocumentCapabilities,
  runtimeNativeSavePlan,
  runtimeSpreadsheetFormatOptions,
} from '@/application/fileProtocol';
import { useDocumentWorkspaceRuntime } from '@/composables/documentWorkspaceRuntime';
import type { DocumentCommandBus } from '@/composables/documentCommandBusAdapter';
import { useDocumentLifecycle } from '@/composables/useDocumentLifecycle';
import { useDocumentReplacementGuard } from '@/composables/useDocumentReplacementGuard';
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
  const workspace = useDocumentWorkspaceRuntime();
  const session = workspace.session;
  const selection = useEditorSelectionStore();
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
  function createCoordinator(
    commandBus: DocumentCommandBus,
    preparations: DocumentPreparationCoordinator,
  ) {
    return createDocumentFileCoordinator({
      getFileData: () => fileData?.value ?? document.data,
      getCommandContext: () => document.currentCommandContext(),
      getCurrentFilePath: () => document.currentFilePath,
      getCurrentSheetIndex: () => selection.currentSheetIndex,
      beginDocumentReplacement,
      runDocumentLifecycle,
      prepareConsistentContext: () => flushPendingCellChanges
        ? commandBus.prepareConsistentContext(flushPendingCellChanges)
          .then((context) => context ?? null)
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
    }, preparations);
  }

  type FileCoordinator = ReturnType<typeof createCoordinator>;
  let admittedCoordinator: FileCoordinator | null = null;

  function runFileTask<T>(
    task: (coordinator: FileCoordinator) => Promise<T>,
    disposedValue: T,
  ): Promise<T> {
    return workspace.runTask(({ commandBus, preparations }) => {
      admittedCoordinator ??= createCoordinator(commandBus, preparations);
      return task(admittedCoordinator);
    }, disposedValue);
  }

  return {
    loadFileFromPath: (...args: Parameters<FileCoordinator['loadFileFromPath']>) =>
      runFileTask((coordinator) => coordinator.loadFileFromPath(...args), false),
    openPickedFile: () =>
      runFileTask((coordinator) => coordinator.openPickedFile(), false),
    openSelectedFile: (...args: Parameters<FileCoordinator['openSelectedFile']>) =>
      runFileTask((coordinator) => coordinator.openSelectedFile(...args), false),
    createNewDocument: () =>
      runFileTask((coordinator) => coordinator.createNewDocument(), false),
    openRecentDocument: (...args: Parameters<FileCoordinator['openRecentDocument']>) =>
      runFileTask((coordinator) => coordinator.openRecentDocument(...args), false),
    saveCurrentFile: () =>
      runFileTask((coordinator) => coordinator.saveCurrentFile(), { status: 'none' as const }),
    exportCurrentFile: () =>
      runFileTask((coordinator) => coordinator.exportCurrentFile(), 'none' as const),
    closeCurrentDocument: (...args: Parameters<FileCoordinator['closeCurrentDocument']>) =>
      runFileTask((coordinator) => coordinator.closeCurrentDocument(...args), false),
    prepareApplicationExit: (...args: Parameters<FileCoordinator['prepareApplicationExit']>) =>
      runFileTask((coordinator) => coordinator.prepareApplicationExit(...args), null),
  };
}
