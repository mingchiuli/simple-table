import * as api from "@/api";
import { useDocumentSessionStore } from "@/stores/documentSession";

export async function restoreActiveDocument(): Promise<boolean> {
  const documentSessionStore = useDocumentSessionStore();
  if (documentSessionStore.data || documentSessionStore.documentId !== null) {
    return false;
  }

  const activeDocument = await api.getActiveDocument();
  if (!activeDocument) {
    return false;
  }

  documentSessionStore.openDocumentResponse(
    activeDocument,
    activeDocument.document.path || null
  );
  return true;
}
