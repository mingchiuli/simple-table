import { discardOpenFileSelection, readFile } from "@/platform";
import type { OpenFileSelection } from "@/platform";
import type { OpenDocumentResponse } from "@/types";

type OpenFileSelectionLifecycleOptions = {
  prepareForDocumentReplacement: () => Promise<boolean>;
};

export function useOpenFileSelection({
  prepareForDocumentReplacement,
}: OpenFileSelectionLifecycleOptions) {
  async function openSelectedFileOrDiscard(
    selection: OpenFileSelection
  ): Promise<OpenDocumentResponse | null> {
    let shouldDiscard = true;
    try {
      if (!(await prepareForDocumentReplacement())) {
        return null;
      }
      const opened = await readFile(selection.path);
      shouldDiscard = false;
      return opened;
    } finally {
      if (shouldDiscard) {
        await discardOpenFileSelection(selection);
      }
    }
  }

  return {
    openSelectedFileOrDiscard,
  };
}
