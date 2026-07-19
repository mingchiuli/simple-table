import {
  defaultHistoryStatus,
  defaultWorkbookCapabilities,
  readyFormulaStatus,
} from '@/types/editorRuntime';
import type {
  DocumentStatusStateInput,
  EditorStateStateInput,
  RuntimeFormulaStatus,
  RuntimeHistoryStatus,
  RuntimeWorkbookCapabilities,
} from '@/types/editorRuntime';

export type DocumentStatusSnapshot = {
  canUndo: boolean;
  canRedo: boolean;
  isContentDirty: boolean;
  hasPendingContentChange: boolean;
  formulaStatus: RuntimeFormulaStatus;
  capabilities: RuntimeWorkbookCapabilities;
  history: RuntimeHistoryStatus;
};

export const useDocumentStatusStore = defineStore("documentStatus", {
  state: () => ({
    canUndo: false,
    canRedo: false,
    isContentDirty: false,
    hasPendingContentChange: false,
    formulaStatus: readyFormulaStatus() as RuntimeFormulaStatus,
    capabilities: defaultWorkbookCapabilities() as RuntimeWorkbookCapabilities,
    history: defaultHistoryStatus() as RuntimeHistoryStatus,
  }),
  getters: {
    hasUnsavedChanges: (state) => state.isContentDirty || state.hasPendingContentChange,
  },
  actions: {
    applyEditorState(state: EditorStateStateInput | null | undefined) {
      this.canUndo = state?.canUndo ?? false;
      this.canRedo = state?.canRedo ?? false;
      this.isContentDirty = state?.isContentDirty ?? false;
      this.history = state?.history ?? defaultHistoryStatus();
    },
    applyRuntimeStatus(
      formulaStatus: RuntimeFormulaStatus,
      capabilities: RuntimeWorkbookCapabilities,
    ) {
      this.formulaStatus = formulaStatus;
      this.capabilities = capabilities;
    },
    applyStatusState(state: DocumentStatusStateInput | null | undefined) {
      if (!state) {
        this.reset();
        return;
      }
      this.canUndo = state.canUndo;
      this.canRedo = state.canRedo;
      this.isContentDirty = state.isContentDirty;
      this.formulaStatus = state.formulaStatus;
      this.capabilities = state.capabilities;
      this.history = state.history;
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
