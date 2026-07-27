export type ActiveDocumentRestorePorts<ActiveDocument> = {
  isFrontendSessionInitialized(): boolean;
  loadActiveDocument(): Promise<ActiveDocument | null>;
  publishActiveDocument(document: ActiveDocument): void;
};

export async function restoreActiveDocument<ActiveDocument>({
  isFrontendSessionInitialized,
  loadActiveDocument,
  publishActiveDocument,
}: ActiveDocumentRestorePorts<ActiveDocument>): Promise<boolean> {
  if (isFrontendSessionInitialized()) return false;

  const activeDocument = await loadActiveDocument();
  if (!activeDocument || isFrontendSessionInitialized()) return false;

  publishActiveDocument(activeDocument);
  return true;
}
