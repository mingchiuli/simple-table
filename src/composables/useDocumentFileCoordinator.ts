import type { ComputedRef } from 'vue';

import * as api from '@/api';
import {
  createDocumentCloseWorkflow,
  type DocumentCloseWorkflowPorts,
} from '@/application/documentCloseWorkflow';
import {
  createDocumentOpenWorkflow,
  type DocumentOpenWorkflowPorts,
} from '@/application/documentOpenWorkflow';
import {
  createDocumentPersistenceWorkflow,
  type DocumentPersistenceWorkflowPorts,
} from '@/application/documentPersistenceWorkflow';
import type { DocumentPreparationCoordinator } from '@/application/documentPreparationCoordinator';
import { createSpreadsheetFormatService } from '@/application/spreadsheetFormatService';
import {
  fileOperationReceiptFromOpenResponse,
  fileOperationReceiptFromSavedResponse,
  runtimeDocumentCapabilities,
  runtimeNativeSavePlan,
  runtimeSpreadsheetFormatOptions,
  savedResponseFromOpenResponse,
} from '@/application/fileProtocol';
import { useDocumentWorkspaceRuntime } from '@/composables/documentWorkspaceRuntime';
import type { DocumentCommandBus } from '@/composables/documentCommandBusAdapter';
import type { OperationCancellationSignal } from '@/application/operationCancellation';
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
import type { OpenDocumentResponse, SavedDocumentResponse } from '@/types/protocol';

type UseDocumentFileCoordinatorOptions = {
  fileData?: ComputedRef<DocumentProjection | null>;
  flushPendingCellChanges?: () => Promise<boolean>;
};

type RuntimeDocumentFilePorts =
  & Omit<DocumentOpenWorkflowPorts<OpenDocumentResponse>, 'closeDocument'>
  & DocumentPersistenceWorkflowPorts<OpenDocumentResponse, SavedDocumentResponse>
  & DocumentCloseWorkflowPorts<OpenDocumentResponse>;

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
    cancellation: OperationCancellationSignal,
  ) {
    const ports: RuntimeDocumentFilePorts = {
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
      prepareOpenFile: (path, preparationId) => prepareOpenFile(path, preparationId),
      prepareRecentFile: (file, preparationId) => prepareRecentFile(file, preparationId),
      prepareNewFile: (preparationId) => api.prepareNewFile(preparationId),
      commitPreparedDocument: (prepared, expectedContext, operationId) =>
        api.commitPreparedDocument(prepared.token, expectedContext, operationId),
      getFileOperationResult: (operationId) => api.getFileOperationResult(operationId),
      getActiveDocument: () => api.getActiveDocument(),
      receiptFromActiveDocument: fileOperationReceiptFromOpenResponse,
      abortPreparedDocument: (preparationId) => api.abortPreparedDocument(preparationId),
      commitCloseDocument: (context, operationId) =>
        api.closeCurrentDocument(context, operationId),
      saveFile: (path, context, operationId) => saveFile(path, context, operationId),
      receiptFromSavedDocument: fileOperationReceiptFromSavedResponse,
      savedDocumentFromActive: savedResponseFromOpenResponse,
      exportFile: (defaultName, context, operationId) => (
        exportFile(defaultName, context, operationId)
      ),
      nativeSavePlan: async (context, target) =>
        runtimeNativeSavePlan(await api.getNativeSavePlan(context, target)),
      documentCapabilities: async (context) =>
        runtimeDocumentCapabilities(await api.getDocumentCapabilities(context)),
      defaultSpreadsheetExtension: spreadsheetFormats.defaultSpreadsheetExtension,
      withReservedSaveLocation,
      openPreparedDocument: (prepared, path) => session.openPreparedDocument(prepared, path),
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
      markDocumentSessionOutcomeUnknown: (context) => {
        session.markProjectionOutcomeUnknown(context);
      },
    };
    const closeWorkflow = createDocumentCloseWorkflow(ports, cancellation);
    const workflowPorts = { ...ports, closeDocument: closeWorkflow.closeDocument };
    return {
      ...createDocumentOpenWorkflow(workflowPorts, preparations, { cancellation }),
      ...createDocumentPersistenceWorkflow(workflowPorts, cancellation),
      closeCurrentDocument: closeWorkflow.closeCurrentDocument,
      prepareApplicationExit: closeWorkflow.prepareApplicationExit,
    };
  }

  type FileCoordinator = ReturnType<typeof createCoordinator>;
  let admittedCoordinator: FileCoordinator | null = null;

  function runFileTask<T>(
    task: (coordinator: FileCoordinator) => Promise<T>,
    disposedValue: T,
  ): Promise<T> {
    return workspace.runTask(({ commandBus, preparations, cancellation }) => {
      admittedCoordinator ??= createCoordinator(commandBus, preparations, cancellation);
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
