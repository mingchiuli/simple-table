import { ElMessageBox } from "element-plus";
import { useDocumentStatusStore } from "@/stores/documentStatus";

export function hasUnsavedDocumentChanges(): boolean {
  return useDocumentStatusStore().hasUnsavedChanges;
}

export async function confirmDiscardUnsavedChanges(): Promise<boolean> {
  if (!hasUnsavedDocumentChanges()) return true;

  try {
    await ElMessageBox.confirm(
      "This document has unsaved changes. Discard them and continue?",
      "Unsaved changes",
      {
        confirmButtonText: "Discard",
        cancelButtonText: "Cancel",
        type: "warning",
      }
    );
    return true;
  } catch {
    return false;
  }
}
