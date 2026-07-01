import { defaultWorkbookCapabilities } from "@/types";
import type { EditorSessionInfo, EditorStateInfo, FormulaStatus, WorkbookCapabilities } from "@/types";

export const useDocumentStatusStore = defineStore("documentStatus", {
  state: () => ({
    canUndo: false,
    canRedo: false,
    isContentDirty: false,
    hasPendingContentChange: false,
    formulaStatus: { state: "ready" } as FormulaStatus,
    capabilities: defaultWorkbookCapabilities() as WorkbookCapabilities,
  }),
  getters: {
    hasUnsavedChanges: (state) => state.isContentDirty || state.hasPendingContentChange,
  },
  actions: {
    applyEditorState(state: EditorStateInfo | null | undefined) {
      this.canUndo = state?.canUndo ?? false;
      this.canRedo = state?.canRedo ?? false;
      this.isContentDirty = state?.isDirty ?? false;
    },
    applyEditorSession(info: EditorSessionInfo | null | undefined) {
      if (!info) {
        this.applyEditorState(null);
        this.capabilities = defaultWorkbookCapabilities();
        return;
      }
      this.formulaStatus = info.formulaStatus;
      this.capabilities = info.capabilities;
      this.applyEditorState(info.editorState);
    },
    reset() {
      this.canUndo = false;
      this.canRedo = false;
      this.isContentDirty = false;
      this.hasPendingContentChange = false;
      this.formulaStatus = { state: "ready" };
      this.capabilities = defaultWorkbookCapabilities();
    },
    markPendingContentChange() {
      this.hasPendingContentChange = true;
    },
    clearPendingContentChange() {
      this.hasPendingContentChange = false;
    },
  },
});
