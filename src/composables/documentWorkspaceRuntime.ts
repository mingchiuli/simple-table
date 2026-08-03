import { hasInjectionContext, inject, type InjectionKey } from 'vue';

import { createDocumentPreparationCoordinator } from '@/application/documentPreparationCoordinator';
import { createDocumentRegionCache } from '@/application/documentRegionCache';
import { createDocumentRegionCoordinator } from '@/application/documentRegionCoordinator';
import { createDocumentSessionCoordinator } from '@/application/documentSessionCoordinator';
import { createPendingCellSaveCoordinator } from '@/application/pendingCellSaveCoordinator';
import { createSearchSessionCoordinator } from '@/application/searchSessionCoordinator';
import {
  createOperationCancellationSource,
  throwIfOperationCancellationFailed,
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
    const cancellationFailures = operationCancellation.cancel();
    sessionWorkflow.discardPendingLocalWork();
    rawSearch.reset();
    disposal = drainAllSettled([
      () => throwIfOperationCancellationFailed(
        cancellationFailures,
        'Failed to notify every document workspace cancellation observer',
      ),
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
    addRow: operations.guard(commandBus.addRow, undefined),
    deleteRow: operations.guard(commandBus.deleteRow, undefined),
    addColumn: operations.guard(commandBus.addColumn, undefined),
    deleteColumn: operations.guard(commandBus.deleteColumn, undefined),
    addSheet: operations.guard(commandBus.addSheet, undefined),
    deleteSheet: operations.guard(commandBus.deleteSheet, undefined),
    undo: operations.guard(commandBus.undo, undefined),
    redo: operations.guard(commandBus.redo, undefined),
    setColumnWidth: operations.guard(commandBus.setColumnWidth, undefined),
    setRowHeight: operations.guard(commandBus.setRowHeight, undefined),
    setCells: operations.guard(commandBus.setCells, undefined),
    search: operations.guard(commandBus.search, undefined),
    refreshAfterMutationError: operations.guard(commandBus.refreshAfterMutationError, false),
    refreshEditorState: operations.guardRequired(commandBus.refreshEditorState),
    ensureSheetLoaded: operations.guard(commandBus.ensureSheetLoaded, false),
    ensureSheetRegionLoaded: operations.guard(commandBus.ensureSheetRegionLoaded, false),
    prepareConsistentContext: operations.guard(commandBus.prepareConsistentContext, undefined),
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
    schedulePendingSave: operations.guardSync(pending.schedulePendingSave, undefined),
    startPendingSave: operations.guardSync(pending.startPendingSave, undefined),
    clearDebounceIfNoQueuedSaves: pending.clearDebounceIfNoQueuedSaves,
    clearPendingContentChangeIfIdle: pending.clearPendingContentChangeIfIdle,
    suspendAutosave: operations.guardSync(pending.suspendAutosave, () => undefined),
    flushPendingCellChanges: operations.guard(pending.flushPendingCellChanges, false),
    waitForInFlightSave: pending.waitForInFlightSave,
    releaseSchedulerIfIdle: pending.releaseSchedulerIfIdle,
    reset: operations.guardSync(pending.reset, undefined),
  };
}

function guardSearchSession(
  search: ReturnType<typeof createSearchSessionCoordinator>,
  operations: WorkspaceOperationTracker,
) {
  return {
    beginSearch: operations.guardSync(search.beginSearch, -1),
    applySearchOutcome: operations.guardSync(search.applySearchOutcome, false),
    finishSearch: operations.guardSync(search.finishSearch, undefined),
    clearSearch: operations.guardSync(search.clearSearch, undefined),
    reset: operations.guardSync(search.reset, undefined),
    captureSnapshot: search.captureSnapshot,
    restoreSnapshot: operations.guardSync(search.restoreSnapshot, undefined),
  };
}
