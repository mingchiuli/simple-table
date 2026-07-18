import { useDocumentStatusStore } from "@/stores/documentStatus";
import { useDocumentSessionCoordinator } from "@/composables/useDocumentSessionCoordinator";
import { usePendingCellSaveCoordinator } from '@/composables/usePendingCellSaveCoordinator';
import { confirmDiscardUnsavedChanges } from "@/composables/unsavedChangesDialog";

type DocumentReplacementGuardOptions = {
  flushPendingCellChanges?: () => Promise<boolean>;
};

export type DocumentReplacementLease = {
  commit: () => void;
  cancel: () => void;
};

export function useDocumentReplacementGuard({
  flushPendingCellChanges,
}: DocumentReplacementGuardOptions = {}) {
  const documentStatusStore = useDocumentStatusStore();
  const documentSessionCoordinator = useDocumentSessionCoordinator();
  const pendingCellSaveCoordinator = usePendingCellSaveCoordinator();

  async function beginDocumentReplacement(): Promise<DocumentReplacementLease | null> {
    if (documentStatusStore.hasUnsavedChanges) {
      return confirmDiscardWithAutosavePaused();
    }
    if (flushPendingCellChanges && !(await flushPendingCellChanges())) {
      return null;
    }
    await documentSessionCoordinator.waitForMutations();
    if (!(await confirmReplacementIfUnsaved())) return null;
    return createReplacementLease();
  }

  async function confirmDiscardWithAutosavePaused(): Promise<DocumentReplacementLease | null> {
    const replacement = createReplacementLease();
    let keepReplacement = false;
    try {
      if (!(await confirmReplacementIfUnsaved())) {
        return null;
      }
      await pendingCellSaveCoordinator.waitForInFlightSave();
      await documentSessionCoordinator.waitForMutations();
      keepReplacement = true;
      return replacement;
    } finally {
      if (!keepReplacement) {
        replacement.cancel();
      }
    }
  }

  async function confirmReplacementIfUnsaved(): Promise<boolean> {
    if (!documentStatusStore.hasUnsavedChanges) return true;
    return confirmDiscardUnsavedChanges();
  }

  function createReplacementLease(): DocumentReplacementLease {
    const resumeAutosave = pendingCellSaveCoordinator.suspendAutosave();
    let settled = false;

    return {
      commit() {
        if (settled) return;
        settled = true;
        documentSessionCoordinator.discardPendingLocalWork();
      },
      cancel() {
        if (settled) return;
        settled = true;
        resumeAutosave();
      },
    };
  }

  return {
    beginDocumentReplacement,
  };
}
