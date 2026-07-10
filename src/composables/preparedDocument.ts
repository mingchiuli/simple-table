import * as api from "@/api";
import type {
  EditorCommandContext,
  OpenDocumentResponse,
  PreparedOpenDocument,
} from "@/types";

export async function commitPreparedDocumentOrAbort(
  prepared: PreparedOpenDocument,
  expectedContext: EditorCommandContext | null
): Promise<OpenDocumentResponse> {
  try {
    return await api.commitPreparedDocument(prepared.token, expectedContext);
  } catch (error) {
    try {
      await api.abortPreparedDocument(prepared.token);
    } catch (cleanupError) {
      console.error("Failed to abort prepared document after commit error:", cleanupError);
    }
    throw error;
  }
}
