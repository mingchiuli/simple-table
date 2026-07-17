import * as api from "@/api";
import { useDocumentSessionCoordinator } from "@/application/documentSessionCoordinator";
import { useDocumentSessionStore } from "@/stores/documentSession";

export async function restoreActiveDocument(): Promise<boolean> {
  const documentSessionStore = useDocumentSessionStore();
  const documentSessionCoordinator = useDocumentSessionCoordinator();
  if (documentSessionStore.data || documentSessionStore.documentId !== null) {
    return false;
  }

  const activeDocument = await api.getActiveDocument();
  if (!activeDocument) {
    return false;
  }

  documentSessionCoordinator.openDocumentResponse(
    activeDocument,
    activeDocument.document.path || null
  );
  return true;
}
