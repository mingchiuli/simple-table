import { defaultHistoryStatus, defaultWorkbookCapabilities, readyFormulaStatus } from "@/types";
import type {
  EditorSessionInfo,
  EditorStateInfo,
  FormulaStatus,
  HistoryStatus,
  WorkbookCapabilities,
} from "@/types";

export const useDocumentStatusStore = defineStore("documentStatus", {
  state: () => ({
    canUndo: false,
    canRedo: false,
    isContentDirty: false,
    hasPendingContentChange: false,
    formulaStatus: readyFormulaStatus() as FormulaStatus,
    capabilities: defaultWorkbookCapabilities() as WorkbookCapabilities,
    history: defaultHistoryStatus() as HistoryStatus,
  }),
  getters: {
    hasUnsavedChanges: (state) => state.isContentDirty || state.hasPendingContentChange,
  },
  actions: {
    applyEditorState(state: EditorStateInfo | null | undefined) {
      this.canUndo = state?.canUndo ?? false;
      this.canRedo = state?.canRedo ?? false;
      this.isContentDirty = state?.isDirty ?? false;
      this.history = state?.history ?? defaultHistoryStatus();
    },
    applyEditorSession(info: EditorSessionInfo | null | undefined) {
      if (!info) {
        this.reset();
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
      this.formulaStatus = readyFormulaStatus();
      this.capabilities = defaultWorkbookCapabilities();
      this.history = defaultHistoryStatus();
    },
    markPendingContentChange() {
      this.hasPendingContentChange = true;
    },
    clearPendingContentChange() {
      this.hasPendingContentChange = false;
    },
  },
});
