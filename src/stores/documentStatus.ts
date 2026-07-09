import { defaultHistoryStatus, defaultWorkbookCapabilities, readyFormulaStatus } from "@/types";
import type {
  EditorSessionInfo,
  EditorStateInfo,
  FormulaStatus,
  HistoryStatus,
  WorkbookCapabilities,
} from "@/types";

export type DocumentStatusSnapshot = {
  canUndo: boolean;
  canRedo: boolean;
  isContentDirty: boolean;
  hasPendingContentChange: boolean;
  formulaStatus: FormulaStatus;
  capabilities: WorkbookCapabilities;
  history: HistoryStatus;
};

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
    applyRuntimeStatus(formulaStatus: FormulaStatus, capabilities: WorkbookCapabilities) {
      this.formulaStatus = formulaStatus;
      this.capabilities = capabilities;
    },
    applyEditorSession(info: EditorSessionInfo | null | undefined) {
      if (!info) {
        this.reset();
        return;
      }
      this.applyRuntimeStatus(info.formulaStatus, info.capabilities);
      this.applyEditorState(info.editorState);
    },
    captureSnapshot(): DocumentStatusSnapshot {
      return {
        canUndo: this.canUndo,
        canRedo: this.canRedo,
        isContentDirty: this.isContentDirty,
        hasPendingContentChange: this.hasPendingContentChange,
        formulaStatus: this.formulaStatus,
        capabilities: this.capabilities,
        history: this.history,
      };
    },
    restoreSnapshot(snapshot: DocumentStatusSnapshot) {
      this.canUndo = snapshot.canUndo;
      this.canRedo = snapshot.canRedo;
      this.isContentDirty = snapshot.isContentDirty;
      this.hasPendingContentChange = snapshot.hasPendingContentChange;
      this.formulaStatus = snapshot.formulaStatus;
      this.capabilities = snapshot.capabilities;
      this.history = snapshot.history;
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
