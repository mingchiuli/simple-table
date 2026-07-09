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
    let actionError: unknown;
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
    } catch (error) {
      actionError = error;
      throw error;
    } finally {
      replacement?.cancel();
      if (shouldDiscard) {
        await discardUnusedOpenFileSelection(selection, actionError);
      }
    }
  }

  return {
    openSelectedFileOrDiscard,
  };
}

async function discardUnusedOpenFileSelection(
  selection: OpenFileSelection,
  originalError?: unknown
) {
  try {
    await discardOpenFileSelection(selection);
  } catch (cleanupError) {
    if (originalError !== undefined) {
      console.error("Failed to discard open file selection after open error:", cleanupError);
      return;
    }
    console.warn("Failed to discard unused open file selection:", cleanupError);
  }
}
