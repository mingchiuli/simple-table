import type { InjectionKey } from 'vue';
import { getCurrentInstance, inject } from 'vue';

import { createDocumentPreparationCoordinator } from '@/application/documentPreparationCoordinator';
import { createDocumentRegionCoordinator } from '@/application/documentRegionCoordinator';
import { createDocumentSessionCoordinator } from '@/application/documentSessionCoordinator';
import { createPendingCellSaveCoordinator } from '@/application/pendingCellSaveCoordinator';
import { createSearchSessionCoordinator } from '@/application/searchSessionCoordinator';
import { createDocumentCommandBus } from '@/composables/documentCommandBusAdapter';
import { useDocumentSessionStore } from '@/stores/documentSession';
import { useDocumentStatusStore } from '@/stores/documentStatus';
import { useEditorSelectionStore } from '@/stores/editorSelection';
import { usePendingCellSavesStore } from '@/stores/pendingCellSaves';
import { useSearchSessionStore } from '@/stores/searchSession';

export type DocumentWorkspaceRuntime = ReturnType<typeof buildDocumentWorkspaceRuntime>;

export const documentWorkspaceRuntimeKey: InjectionKey<DocumentWorkspaceRuntime> =
  Symbol('document-workspace-runtime');

const runtimes = new WeakMap<object, DocumentWorkspaceRuntime>();

export function createDocumentWorkspaceRuntime(): DocumentWorkspaceRuntime {
  const document = useDocumentSessionStore();
  const existing = runtimes.get(document);
  if (existing) return existing;

  const runtime = buildDocumentWorkspaceRuntime(document);
  runtimes.set(document, runtime);
  return runtime;
}

export function useDocumentWorkspaceRuntime(): DocumentWorkspaceRuntime {
  if (getCurrentInstance()) {
    const provided = inject(documentWorkspaceRuntimeKey, null);
    if (provided) return provided;
  }
  return createDocumentWorkspaceRuntime();
}

function buildDocumentWorkspaceRuntime(
  document: ReturnType<typeof useDocumentSessionStore>,
) {
  const status = useDocumentStatusStore();
  const selection = useEditorSelectionStore();
  const pendingCellSaves = createPendingCellSaveCoordinator(usePendingCellSavesStore());
  const search = createSearchSessionCoordinator(useSearchSessionStore());
  const regions = createDocumentRegionCoordinator(document);
  const sessionWorkflow = createDocumentSessionCoordinator({
    document,
    status,
    selection,
    pending: pendingCellSaves,
    search,
    regions,
  });
  const session = {
    ...sessionWorkflow,
    ensureSheetLoaded: regions.ensureSheetLoaded,
    ensureSheetRegionLoaded: regions.ensureSheetRegionLoaded,
  };
  const preparations = createDocumentPreparationCoordinator();
  const commandBus = createDocumentCommandBus(document, session, selection);
  let disposal: Promise<void> | null = null;

  function dispose(): Promise<void> {
    if (disposal) return disposal;
    sessionWorkflow.discardPendingLocalWork();
    search.reset();
    disposal = Promise.all([
      preparations.waitForIdle(),
      sessionWorkflow.waitForMutations(),
      pendingCellSaves.waitForInFlightSave(),
      regions.waitForIdle(),
    ]).then(() => {
      runtimes.delete(document);
    });
    return disposal;
  }

  return {
    document,
    session,
    pendingCellSaves,
    search,
    preparations,
    commandBus,
    dispose,
  };
}
