import { ElMessageBox } from "element-plus";

export async function confirmDiscardUnsavedChanges(): Promise<boolean> {
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
