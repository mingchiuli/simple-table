import type { EditorSessionInfo, EditorStateInfo, FormulaStatus } from "@/types";

export const useDocumentStatusStore = defineStore("documentStatus", {
  state: () => ({
    canUndo: false,
    canRedo: false,
    isContentDirty: false,
    hasPendingContentChange: false,
    formulaStatus: { state: "ready" } as FormulaStatus,
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
        return;
      }
      this.formulaStatus = info.formulaStatus;
      this.applyEditorState(info.editorState);
    },
    reset() {
      this.canUndo = false;
      this.canRedo = false;
      this.isContentDirty = false;
      this.hasPendingContentChange = false;
      this.formulaStatus = { state: "ready" };
    },
    markPendingContentChange() {
      this.hasPendingContentChange = true;
    },
    clearPendingContentChange() {
      this.hasPendingContentChange = false;
    },
  },
});
