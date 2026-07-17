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
import { openResponseFromFileData } from "@/test/documentFixtures";
import { sheetCell } from "@/stores/documentProjection";
import { useDocumentSessionCoordinator } from "@/composables/useDocumentSessionCoordinator";

vi.mock("element-plus", () => ({
  ElMessage: {
    error: vi.fn(),
  },
}));

vi.mock("@/api", () => ({
  setCells: vi.fn(),
  getCurrentDocumentProjection: vi.fn(),
  getEditorState: vi.fn(),
  getActiveDocument: vi.fn(),
  getMutationResult: vi.fn().mockResolvedValue({ status: 'missing' }),
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

function editorSession(revision: number | string): EditorSessionInfo {
  return {
    documentId: '1',
    revision: String(revision) as `${bigint}`,
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
    protocolVersion: 4,
    documentId: '1',
    revision: '3',
    formulaStatus: readyFormulaStatus(),
    capabilities: defaultWorkbookCapabilities(),
    editorState: {
      canUndo: true,
      canRedo: false,
      isDirty: true,
      history: defaultHistoryStatus(),
    },
    patches: [],
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
    const documentSessionCoordinator = useDocumentSessionCoordinator();
    documentSessionCoordinator.openDocumentResponse(
      openResponseFromFileData(fileData("old"), editorSession(0)),
      "/tmp/book.xlsx"
    );
    const fresh = fileData("draft");
    vi.mocked(api.setCells).mockResolvedValue(mutationResponse({ revision: '3' }));
    vi.mocked(api.getCurrentDocumentProjection)
      .mockRejectedValueOnce(new Error("projection unavailable"))
      .mockResolvedValueOnce(openResponseFromFileData(fresh, editorSession(3)));
    vi.mocked(api.getEditorState).mockResolvedValue(editorSession(3));

    const scope = effectScope();
    const controller = scope.run(() => {
      const file = computed(() => documentSessionStore.data);
      const currentSheet = computed(() => documentSessionStore.loadedSheet(0));
      const selectedCell = ref({ row: 0, col: 0 });
      return useCellEditController({
        fileData: file,
        currentSheet,
        currentSheetIndex: ref(0),
        selectedCell,
        cellEditorValue: ref(""),
        canEditCells: computed(() => true),
      });
    });

    if (!controller) {
      throw new Error("controller setup failed");
    }
    controller.handleCellEditing(0, 0, "draft");
    await expect(controller.flushPendingCellChanges()).resolves.toBe(true);

    expect(api.setCells).toHaveBeenCalledWith(
      expect.objectContaining({
        documentId: '1', baseRevision: '0', commandId: expect.any(String),
      }),
      [{ sheetIndex: 0, row: 0, col: 0, text: "draft" }]
    );
    expect(api.getCurrentDocumentProjection).toHaveBeenCalledTimes(2);
    expect(documentSessionStore.revision).toBe('3');
    expect(documentSessionStore.projectionStale).toBe(false);
    expect(sheetCell(documentSessionStore.data?.sheets[0], 0, 0)).toEqual(text("draft"));
    expect(usePendingCellSavesStore().phase).toBe("idle");
    expect(usePendingCellSavesStore().draftCellValues.size).toBe(0);
    expect(statusStore.hasPendingContentChange).toBe(false);
    expect(elementPlus.ElMessage.error).not.toHaveBeenCalled();

    scope.stop();
  });

  it("locks the editor as stale when backend cell save succeeds but frontend apply and refresh fail", async () => {
    const api = await import("@/api");
    const elementPlus = await import("element-plus");
    const documentSessionStore = useDocumentSessionStore();
    const statusStore = useDocumentStatusStore();
    const documentSessionCoordinator = useDocumentSessionCoordinator();
    documentSessionCoordinator.openDocumentResponse(
      openResponseFromFileData(fileData("old"), editorSession(0)),
      "/tmp/book.xlsx"
    );
    vi.mocked(api.setCells).mockResolvedValue(mutationResponse({ revision: '3' }));
    vi.mocked(api.getCurrentDocumentProjection)
      .mockRejectedValue(new Error("projection unavailable"));
    vi.mocked(api.getEditorState).mockRejectedValue(new Error("state unavailable"));
    const applyMutationResponse = vi.fn().mockRejectedValue(new Error("frontend apply failed"));
    vi.spyOn(documentSessionCoordinator, "applyMutationResponseWithResync")
      .mockImplementation(applyMutationResponse);

    const scope = effectScope();
    const controller = scope.run(() => {
      const file = computed(() => documentSessionStore.data);
      const currentSheet = computed(() => documentSessionStore.loadedSheet(0));
      const selectedCell = ref({ row: 0, col: 0 });
      return useCellEditController({
        fileData: file,
        currentSheet,
        currentSheetIndex: ref(0),
        selectedCell,
        cellEditorValue: ref(""),
        canEditCells: computed(() => true),
      });
    });

    if (!controller) {
      throw new Error("controller setup failed");
    }
    controller.handleCellEditing(0, 0, "draft");
    await expect(controller.flushPendingCellChanges()).resolves.toBe(true);

    expect(api.setCells).toHaveBeenCalledWith(
      expect.objectContaining({
        documentId: '1', baseRevision: '0', commandId: expect.any(String),
      }),
      [{ sheetIndex: 0, row: 0, col: 0, text: "draft" }]
    );
    expect(api.getCurrentDocumentProjection).toHaveBeenCalledWith(
      { documentId: '1', baseRevision: '3' },
      0
    );
    expect(api.getEditorState).toHaveBeenCalledWith({ documentId: '1', baseRevision: '3' });
    expect(documentSessionStore.revision).toBe('3');
    expect(documentSessionStore.projectionStale).toBe(true);
    expect(documentSessionStore.isEditorInteractionLocked).toBe(true);
    expect(sheetCell(documentSessionStore.data?.sheets[0], 0, 0)).toEqual(text("old"));
    expect(usePendingCellSavesStore().phase).toBe("idle");
    expect(usePendingCellSavesStore().draftCellValues.size).toBe(0);
    expect(statusStore.hasPendingContentChange).toBe(false);
    expect(elementPlus.ElMessage.error).toHaveBeenCalledWith(
      "保存已提交，但刷新失败: Error: frontend apply failed"
    );

    scope.stop();
  });
});
