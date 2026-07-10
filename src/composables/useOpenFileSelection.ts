import { discardOpenFileSelection, prepareOpenFile } from "@/platform";
import type { OpenFileSelection } from "@/platform";
import type { DocumentReplacementLease } from "@/composables/useDocumentReplacementGuard";
import { useDocumentSessionStore } from "@/stores/documentSession";
import { commitPreparedDocumentOrAbort } from "@/composables/preparedDocument";

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
      const expectedContext = documentSessionStore.currentCommandContext();
      const prepared = await prepareOpenFile(selection.path);
      const opened = await commitPreparedDocumentOrAbort(prepared, expectedContext);
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
