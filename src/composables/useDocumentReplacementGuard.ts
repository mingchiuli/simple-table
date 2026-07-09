import { useDocumentSessionStore } from "@/stores/documentSession";
import { usePendingCellSavesStore } from "@/stores/pendingCellSaves";
import {
  confirmDiscardUnsavedChanges,
  hasUnsavedDocumentChanges,
} from "@/utils/unsavedChanges";

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
  const documentSessionStore = useDocumentSessionStore();
  const pendingCellSavesStore = usePendingCellSavesStore();

  async function beginDocumentReplacement(): Promise<DocumentReplacementLease | null> {
    if (hasUnsavedDocumentChanges()) {
      if (!(await confirmReplacementIfUnsaved())) return null;
      return createReplacementLease();
    }
    if (flushPendingCellChanges && !(await flushPendingCellChanges())) {
      return null;
    }
    await documentSessionStore.waitForMutations();
    if (!(await confirmReplacementIfUnsaved())) return null;
    return createReplacementLease();
  }

  async function confirmReplacementIfUnsaved(): Promise<boolean> {
    if (!hasUnsavedDocumentChanges()) return true;
    return confirmDiscardUnsavedChanges();
  }

  function createReplacementLease(): DocumentReplacementLease {
    const resumeAutosave = pendingCellSavesStore.suspendAutosave();
    let settled = false;

    return {
      commit() {
        if (settled) return;
        settled = true;
        documentSessionStore.discardPendingLocalWork();
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
