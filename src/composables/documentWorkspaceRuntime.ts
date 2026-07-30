import { hasInjectionContext, inject, type InjectionKey } from 'vue';

import { createDocumentPreparationCoordinator } from '@/application/documentPreparationCoordinator';
import { createDocumentRegionCache } from '@/application/documentRegionCache';
import { createDocumentRegionCoordinator } from '@/application/documentRegionCoordinator';
import { createDocumentSessionCoordinator } from '@/application/documentSessionCoordinator';
import { createPendingCellSaveCoordinator } from '@/application/pendingCellSaveCoordinator';
import { createSearchSessionCoordinator } from '@/application/searchSessionCoordinator';
import {
  createOperationCancellationSource,
  type OperationCancellationSignal,
} from '@/application/operationCancellation';
import {
  createDocumentCommandBus,
  type DocumentCommandBus,
} from '@/composables/documentCommandBusAdapter';
import { createDocumentSessionStoreAdapter } from '@/composables/documentSessionStoreAdapter';
import {
  createWorkspaceOperationTracker,
  type WorkspaceOperationTracker,
} from '@/application/workspaceOperationTracker';
import { drainAllSettled } from '@/application/asyncDrain';
import { useDocumentSessionStore } from '@/stores/documentSession';
import { useDocumentStatusStore } from '@/stores/documentStatus';
import { useEditorSelectionStore } from '@/stores/editorSelection';
import { usePendingCellSavesStore } from '@/stores/pendingCellSaves';
import { useSearchSessionStore } from '@/stores/searchSession';

export type DocumentWorkspaceRuntime = ReturnType<typeof buildDocumentWorkspaceRuntime>;

export const documentWorkspaceRuntimeKey: InjectionKey<DocumentWorkspaceRuntime> =
  Symbol('document-workspace-runtime');

export function createDocumentWorkspaceRuntime(): DocumentWorkspaceRuntime {
  const document = useDocumentSessionStore();
  return buildDocumentWorkspaceRuntime(document);
}

export function useDocumentWorkspaceRuntime(): DocumentWorkspaceRuntime {
  if (!hasInjectionContext()) {
    throw new Error('Document workspace runtime must be provided by the application root');
  }
  const runtime = inject(documentWorkspaceRuntimeKey, null);
  if (!runtime) {
    throw new Error('Document workspace runtime must be provided by the application root');
  }
  return runtime;
}

function buildDocumentWorkspaceRuntime(
  document: ReturnType<typeof useDocumentSessionStore>,
) {
  const operations = createWorkspaceOperationTracker();
  const operationCancellation = createOperationCancellationSource();
  const status = useDocumentStatusStore();
  const selection = useEditorSelectionStore();
  const rawPendingCellSaves = createPendingCellSaveCoordinator(usePendingCellSavesStore());
  const rawSearch = createSearchSessionCoordinator(useSearchSessionStore());
  const regionCache = createDocumentRegionCache(document);
  const documentSession = createDocumentSessionStoreAdapter(document, regionCache);
  const regions = createDocumentRegionCoordinator(regionCache, operationCancellation.signal);
  const sessionWorkflow = createDocumentSessionCoordinator({
    document: documentSession,
    status,
    selection,
    pending: rawPendingCellSaves,
    search: rawSearch,
    regions,
  });
  const session = {
    ...sessionWorkflow,
    ensureSheetLoaded: regions.ensureSheetLoaded,
    ensureSheetRegionLoaded: regions.ensureSheetRegionLoaded,
  };
  const rawPreparations = createDocumentPreparationCoordinator({
    reportCleanupFailure: (error) => {
      console.error('Failed to clean up cancelled document preparation:', error);
    },
  });
  const rawCommandBus = createDocumentCommandBus(
    document,
    session,
    selection,
    operationCancellation.signal,
  );
  const pendingCellSaves = guardPendingCellSaves(rawPendingCellSaves, operations);
  const search = guardSearchSession(rawSearch, operations);
  const preparations = guardDocumentPreparations(rawPreparations, operations);
  const commandBus = guardDocumentCommandBus(rawCommandBus, operations);
  const admittedServices = {
    commandBus: rawCommandBus,
    preparations: rawPreparations,
    cancellation: operationCancellation.signal,
  };
  let disposal: Promise<void> | null = null;

  function runTask<T>(
    task: (services: typeof admittedServices) => Promise<T>,
    disposedValue: T,
  ): Promise<T> {
    return operations.run(() => task(admittedServices), disposedValue);
  }

  function dispose(): Promise<void> {
    if (disposal) return disposal;
    operations.stopAcceptingWork();
    operationCancellation.cancel();
    sessionWorkflow.discardPendingLocalWork();
    rawSearch.reset();
    disposal = drainAllSettled([
      () => operations.waitForIdle(),
      () => rawPreparations.waitForIdle(),
      () => sessionWorkflow.waitForMutations(),
      () => rawPendingCellSaves.waitForInFlightSave(),
      () => regions.waitForIdle(),
    ], 'Failed to completely drain the document workspace').finally(() => {
      operations.markDisposed();
    });
    return disposal;
  }

  return {
    document,
    documentSession,
    regionCache,
    session,
    pendingCellSaves,
    search,
    preparations,
    commandBus,
    runTask,
    runRequiredTask: operations.runRequired,
    dispose,
  };
}

function guardDocumentCommandBus(
  commandBus: DocumentCommandBus,
  operations: WorkspaceOperationTracker,
): DocumentCommandBus {
  return {
    addRow: (...args) => operations.run(() => commandBus.addRow(...args), undefined),
    deleteRow: (...args) => operations.run(() => commandBus.deleteRow(...args), undefined),
    addColumn: (...args) => operations.run(() => commandBus.addColumn(...args), undefined),
    deleteColumn: (...args) => operations.run(() => commandBus.deleteColumn(...args), undefined),
    addSheet: (...args) => operations.run(() => commandBus.addSheet(...args), undefined),
    deleteSheet: (...args) => operations.run(() => commandBus.deleteSheet(...args), undefined),
    undo: (...args) => operations.run(() => commandBus.undo(...args), undefined),
    redo: (...args) => operations.run(() => commandBus.redo(...args), undefined),
    setColumnWidth: (...args) =>
      operations.run(() => commandBus.setColumnWidth(...args), undefined),
    setRowHeight: (...args) =>
      operations.run(() => commandBus.setRowHeight(...args), undefined),
    setCells: (...args) => operations.run(() => commandBus.setCells(...args), undefined),
    search: (...args) => operations.run(() => commandBus.search(...args), undefined),
    refreshAfterMutationError: (...args) =>
      operations.run(() => commandBus.refreshAfterMutationError(...args), false),
    refreshEditorState: (...args) =>
      operations.runRequired(() => commandBus.refreshEditorState(...args)),
    ensureSheetLoaded: (...args) =>
      operations.run(() => commandBus.ensureSheetLoaded(...args), false),
    ensureSheetRegionLoaded: (...args) =>
      operations.run(() => commandBus.ensureSheetRegionLoaded(...args), false),
    prepareConsistentContext: (...args) =>
      operations.run(() => commandBus.prepareConsistentContext(...args), undefined),
  };
}

function guardDocumentPreparations(
  preparations: ReturnType<typeof createDocumentPreparationCoordinator>,
  operations: WorkspaceOperationTracker,
) {
  return {
    run<T>(prepare: () => Promise<T>) {
      return operations.runRequired(() => preparations.run(prepare));
    },
    runCancellable<T>(
      prepare: () => Promise<T>,
      cancellation: OperationCancellationSignal,
      discard: (result: T) => Promise<void>,
    ) {
      return operations.runRequired(() =>
        preparations.runCancellable(prepare, cancellation, discard));
    },
    cleanup: preparations.cleanup,
    cleanupPreparationId: preparations.cleanupPreparationId,
    drainPreparationCleanupIds: preparations.drainPreparationCleanupIds,
    waitForIdle: preparations.waitForIdle,
  };
}

function guardPendingCellSaves(
  pending: ReturnType<typeof createPendingCellSaveCoordinator>,
  operations: WorkspaceOperationTracker,
) {
  return {
    hasPendingWork: pending.hasPendingWork,
    schedulePendingSave: (...args: Parameters<typeof pending.schedulePendingSave>) => {
      if (operations.isAcceptingWork()) pending.schedulePendingSave(...args);
    },
    startPendingSave: (...args: Parameters<typeof pending.startPendingSave>) => {
      if (operations.isAcceptingWork()) pending.startPendingSave(...args);
    },
    clearDebounceIfNoQueuedSaves: pending.clearDebounceIfNoQueuedSaves,
    clearPendingContentChangeIfIdle: pending.clearPendingContentChangeIfIdle,
    suspendAutosave: () => operations.isAcceptingWork()
      ? pending.suspendAutosave()
      : () => undefined,
    flushPendingCellChanges: (...args: Parameters<typeof pending.flushPendingCellChanges>) =>
      operations.run(() => pending.flushPendingCellChanges(...args), false),
    waitForInFlightSave: pending.waitForInFlightSave,
    releaseSchedulerIfIdle: pending.releaseSchedulerIfIdle,
    reset: () => {
      if (operations.isAcceptingWork()) pending.reset();
    },
  };
}

function guardSearchSession(
  search: ReturnType<typeof createSearchSessionCoordinator>,
  operations: WorkspaceOperationTracker,
) {
  return {
    beginSearch: (query: string) => operations.isAcceptingWork() ? search.beginSearch(query) : -1,
    applySearchOutcome: (...args: Parameters<typeof search.applySearchOutcome>) =>
      operations.isAcceptingWork() && search.applySearchOutcome(...args),
    finishSearch: (...args: Parameters<typeof search.finishSearch>) => {
      if (operations.isAcceptingWork()) search.finishSearch(...args);
    },
    clearSearch: () => {
      if (operations.isAcceptingWork()) search.clearSearch();
    },
    reset: () => {
      if (operations.isAcceptingWork()) search.reset();
    },
    captureSnapshot: search.captureSnapshot,
    restoreSnapshot: (...args: Parameters<typeof search.restoreSnapshot>) => {
      if (operations.isAcceptingWork()) search.restoreSnapshot(...args);
    },
  };
}
