import { useDocumentSessionStore } from "@/stores/documentSession";
import {
  confirmDiscardUnsavedChanges,
  hasUnsavedDocumentChanges,
} from "@/utils/unsavedChanges";

type DocumentReplacementGuardOptions = {
  flushPendingCellChanges?: () => Promise<boolean>;
};

export function useDocumentReplacementGuard({
  flushPendingCellChanges,
}: DocumentReplacementGuardOptions = {}) {
  const documentSessionStore = useDocumentSessionStore();

  async function prepareForDocumentReplacement(): Promise<boolean> {
    if (hasUnsavedDocumentChanges()) {
      return confirmAndDiscardIfUnsaved();
    }
    if (flushPendingCellChanges && !(await flushPendingCellChanges())) {
      return false;
    }
    await documentSessionStore.waitForMutations();
    return confirmAndDiscardIfUnsaved();
  }

  async function confirmAndDiscardIfUnsaved(): Promise<boolean> {
    if (!hasUnsavedDocumentChanges()) return true;
    if (!(await confirmDiscardUnsavedChanges())) return false;
    documentSessionStore.discardPendingLocalWork();
    return true;
  }

  return {
    prepareForDocumentReplacement,
  };
}
