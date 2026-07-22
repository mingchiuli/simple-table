import * as api from "@/api";
import type { DocumentWorkspaceRuntime } from '@/composables/documentWorkspaceRuntime';

export async function restoreActiveDocument(
  workspace: DocumentWorkspaceRuntime,
): Promise<boolean> {
  if (workspace.document.data || workspace.document.documentId !== null) {
    return false;
  }

  const activeDocument = await api.getActiveDocument();
  if (!activeDocument) {
    return false;
  }

  workspace.session.openDocumentResponse(
    activeDocument,
    activeDocument.document.path || null
  );
  return true;
}
