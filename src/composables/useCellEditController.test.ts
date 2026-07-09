import { beforeEach, describe, expect, it, vi } from "vitest";
import { computed, effectScope, ref } from "vue";
import { createPinia, setActivePinia } from "pinia";
import { useCellEditController } from "@/composables/useCellEditController";
import { useDocumentSessionStore } from "@/stores/documentSession";
import { useDocumentStatusStore } from "@/stores/documentStatus";
import { usePendingCellSavesStore } from "@/stores/pendingCellSaves";
import {
  defaultHistoryStatus,
  defaultRichProjection,
  defaultWorkbookCapabilities,
  readyFormulaStatus,
  type CellValue,
  type EditorMutationResponse,
  type EditorSessionInfo,
  type FileData,
  type SheetData,
} from "@/types";

vi.mock("element-plus", () => ({
  ElMessage: {
    error: vi.fn(),
  },
}));

vi.mock("@/api", () => ({
  setCells: vi.fn(),
  getCurrentFileData: vi.fn(),
  getEditorState: vi.fn(),
}));

function text(value: string): CellValue {
  return { type: "cell", kind: "text", raw: value, display: value };
}

function sheet(name: string, rows: CellValue[][]): SheetData {
  return { name, rows, merges: [], rich: defaultRichProjection() };
}

function fileData(value: string): FileData {
  return {
    path: "/tmp/book.xlsx",
    fileName: "book.xlsx",
    sheets: [sheet("Sheet1", [[text(value)]])],
  };
}

function editorSession(revision: number): EditorSessionInfo {
  return {
    documentId: 1,
    revision,
    formulaStatus: readyFormulaStatus(),
    capabilities: defaultWorkbookCapabilities(),
    editorState: {
      canUndo: true,
      canRedo: false,
      isDirty: true,
      history: defaultHistoryStatus(),
    },
  };
}

function mutationResponse(partial: Partial<EditorMutationResponse> = {}): EditorMutationResponse {
  return {
    protocolVersion: 1,
    documentId: 1,
    revision: 3,
    formulaStatus: readyFormulaStatus(),
    capabilities: defaultWorkbookCapabilities(),
    editorState: {
      canUndo: true,
      canRedo: false,
      isDirty: true,
      history: defaultHistoryStatus(),
    },
    patches: [],
    searchIndexUpdate: { rebuildAll: false, rebuildSheets: [] },
    ...partial,
  };
}

describe("useCellEditController", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
  });

  it("treats backend cell save success as committed when projection resync is recovered", async () => {
    const api = await import("@/api");
    const elementPlus = await import("element-plus");
    const documentSessionStore = useDocumentSessionStore();
    const statusStore = useDocumentStatusStore();
    documentSessionStore.openDocumentResponse({
      fileData: fileData("old"),
      editorSession: editorSession(0),
    }, "/tmp/book.xlsx");
    const fresh = fileData("draft");
    vi.mocked(api.setCells).mockResolvedValue(mutationResponse({ revision: 3 }));
    vi.mocked(api.getCurrentFileData)
      .mockRejectedValueOnce(new Error("projection unavailable"))
      .mockResolvedValueOnce(fresh);
    vi.mocked(api.getEditorState).mockResolvedValue(editorSession(3));

    const scope = effectScope();
    const controller = scope.run(() => {
      const file = computed(() => documentSessionStore.data);
      const currentSheet = computed(() => file.value?.sheets[0] ?? null);
      const selectedCell = ref({ row: 0, col: 0 });
      return useCellEditController({
        fileData: file,
        currentSheet,
        currentSheetIndex: ref(0),
        selectedCell,
        cellEditorValue: ref(""),
        canEditCells: computed(() => true),
        applyMutationResponse: (response) =>
          documentSessionStore.applyMutationResponseWithResync(
            response,
            api.getCurrentFileData
          ).then(() => undefined),
        markPendingContentChange: () => statusStore.markPendingContentChange(),
        clearPendingContentChange: () => statusStore.clearPendingContentChange(),
      });
    });

    if (!controller) {
      throw new Error("controller setup failed");
    }
    controller.handleCellEditing(0, 0, "draft");
    await expect(controller.flushPendingCellChanges()).resolves.toBe(true);

    expect(api.setCells).toHaveBeenCalledWith(
      { documentId: 1, baseRevision: 0 },
      [{ sheetIndex: 0, row: 0, col: 0, text: "draft" }]
    );
    expect(api.getCurrentFileData).toHaveBeenCalledTimes(2);
    expect(documentSessionStore.revision).toBe(3);
    expect(documentSessionStore.projectionStale).toBe(false);
    expect(documentSessionStore.data?.sheets[0].rows[0][0]).toEqual(text("draft"));
    expect(usePendingCellSavesStore().phase).toBe("idle");
    expect(usePendingCellSavesStore().draftCellValues.size).toBe(0);
    expect(statusStore.hasPendingContentChange).toBe(false);
    expect(elementPlus.ElMessage.error).not.toHaveBeenCalled();

    scope.stop();
  });
});
