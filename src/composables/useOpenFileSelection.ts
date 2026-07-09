import { discardOpenFileSelection, readFile } from "@/platform";
import type { OpenFileSelection } from "@/platform";
import type { DocumentReplacementLease } from "@/composables/useDocumentReplacementGuard";
import { useDocumentSessionStore } from "@/stores/documentSession";

type OpenFileSelectionLifecycleOptions = {
  beginDocumentReplacement: () => Promise<DocumentReplacementLease | null>;
};

export function useOpenFileSelection({
  beginDocumentReplacement,
}: OpenFileSelectionLifecycleOptions) {
  const documentSessionStore = useDocumentSessionStore();

  async function openSelectedFileOrDiscard(selection: OpenFileSelection): Promise<boolean> {
    let shouldDiscard = true;
    let replacement: DocumentReplacementLease | null = null;
    try {
      replacement = await beginDocumentReplacement();
      if (!replacement) {
        return false;
      }
      const opened = await readFile(selection.path);
      shouldDiscard = false;
      replacement.commit();
      replacement = null;
      documentSessionStore.openDocumentResponse(opened, selection.path);
      return true;
    } finally {
      replacement?.cancel();
      if (shouldDiscard) {
        await discardOpenFileSelection(selection);
      }
    }
  }

  return {
    openSelectedFileOrDiscard,
  };
}
