import { beforeEach, describe, expect, it, vi } from "vitest";
import { computed, type Ref } from "vue";
import { createPinia, setActivePinia, storeToRefs } from "pinia";
import { useEditorCommands } from "@/composables/useEditorCommands";
import { useDocumentSessionStore } from "@/stores/documentSession";
import { useEditorSelectionStore } from "@/stores/editorSelection";
import {
  defaultHistoryStatus,
  defaultRichProjection,
  defaultWorkbookCapabilities,
  readyFormulaStatus,
  type CellValue,
  type EditorMutationResponse,
  type FileData,
  type OpenDocumentResponse,
  type SearchResult,
  type SheetData,
} from "@/types";

vi.mock("element-plus", () => ({
  ElMessage: {
    error: vi.fn(),
    warning: vi.fn(),
  },
}));

vi.mock("@/api", () => ({
  addRow: vi.fn(),
  addSheet: vi.fn(),
  getCurrentFileData: vi.fn(),
  getEditorState: vi.fn(),
  search: vi.fn().mockResolvedValue([]),
}));

function text(value: string): CellValue {
  return { type: "cell", kind: "text", raw: value, display: value };
}

function sheet(name: string, rows: CellValue[][]): SheetData {
  return { name, rows, merges: [], rich: defaultRichProjection() };
}

function openedResponse(documentId: number | string = '1', fileName = "book.xlsx"): OpenDocumentResponse {
  const fileData: FileData = {
    path: `/tmp/${fileName}`,
    fileName,
    sheets: [
      sheet("Sheet1", [[text("A1")]]),
      sheet("Sheet2", [[text("B1")]]),
    ],
  };
  return {
    fileData,
    editorSession: {
      documentId: String(documentId),
      revision: '0',
      formulaStatus: readyFormulaStatus(),
      capabilities: defaultWorkbookCapabilities(),
      editorState: {
        canUndo: true,
        canRedo: true,
        isDirty: false,
        history: defaultHistoryStatus(),
      },
    },
  };
}

function mutationResponse(partial: Partial<EditorMutationResponse> = {}): EditorMutationResponse {
  return {
    protocolVersion: 1,
    documentId: '1',
    revision: '1',
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

function setupCommands(
  flushPendingCellChanges = vi.fn().mockResolvedValue(true),
  overrides: { currentSheetIndex?: Ref<number> } = {}
) {
  const documentSessionStore = useDocumentSessionStore();
  documentSessionStore.openDocumentResponse(openedResponse(), "/tmp/book.xlsx");
  const selectionStore = useEditorSelectionStore();
  const { currentSheetIndex, selectedCell } = storeToRefs(selectionStore);
  const activeSheetIndex = overrides.currentSheetIndex ?? currentSheetIndex;
  const fileData = computed(() => documentSessionStore.data);
  const currentSheet = computed(() => fileData.value?.sheets[activeSheetIndex.value] ?? null);
  const applyMutationResponse = vi.fn(async (_response: EditorMutationResponse) => ({
    data: documentSessionStore.data,
    resyncRequired: false,
    applied: true,
  }));

  const commands = useEditorCommands({
    fileData,
    currentSheet,
    currentSheetIndex: activeSheetIndex,
    selectedCell,
    flushPendingCellChanges,
    editorValueForCell: () => "",
    applyMutationResponse,
  });

  return {
    commands,
    documentSessionStore,
    selectionStore,
    currentSheetIndex: activeSheetIndex,
    selectedCell,
    flushPendingCellChanges,
    applyMutationResponse,
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((promiseResolve, promiseReject) => {
    resolve = promiseResolve;
    reject = promiseReject;
  });
  return { promise, resolve, reject };
}

describe("useEditorCommands", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
  });

  it("does not start structural mutations while another editor command is running", async () => {
    const api = await import("@/api");
    const { commands, flushPendingCellChanges, documentSessionStore } = setupCommands();
    documentSessionStore.beginEditorCommand();

    await commands.handleAddRow();

    expect(api.addRow).not.toHaveBeenCalled();
    expect(flushPendingCellChanges).not.toHaveBeenCalled();
  });

  it("does not switch sheets while another editor command is running", () => {
    const { commands, currentSheetIndex, documentSessionStore } = setupCommands();
    documentSessionStore.beginEditorCommand();

    commands.handleSheetChange(1);

    expect(currentSheetIndex.value).toBe(0);
  });

  it("does not change cell selection while another editor command is running", () => {
    const { commands, selectedCell, documentSessionStore } = setupCommands();
    documentSessionStore.beginEditorCommand();

    commands.handleSelectCell(0, 0);

    expect(selectedCell.value).toBeNull();
  });

  it("does not navigate to search results while another editor command is running", () => {
    const { commands, currentSheetIndex, selectedCell, documentSessionStore } = setupCommands();
    documentSessionStore.beginEditorCommand();
    const result: SearchResult = {
      sheetIndex: 1,
      sheetName: "Sheet2",
      row: 0,
      col: 0,
      value: "B1",
      cellPosition: "A1",
    };

    commands.handleSearchResultClick(result);

    expect(currentSheetIndex.value).toBe(0);
    expect(selectedCell.value).toBeNull();
  });

  it("skips queued structural mutations when the document changes while flushing edits", async () => {
    const api = await import("@/api");
    let documentSessionStore!: ReturnType<typeof useDocumentSessionStore>;
    const flushPendingCellChanges = vi.fn().mockImplementation(async () => {
      documentSessionStore.openDocumentResponse(openedResponse(2, "next.xlsx"), "/tmp/next.xlsx");
      return true;
    });
    const setup = setupCommands(flushPendingCellChanges);
    documentSessionStore = setup.documentSessionStore;

    await setup.commands.handleAddRow();

    expect(api.addRow).not.toHaveBeenCalled();
    expect(setup.applyMutationResponse).not.toHaveBeenCalled();
    expect(documentSessionStore.documentId).toBe('2');
  });

  it("skips search when the document changes while flushing edits", async () => {
    const api = await import("@/api");
    let documentSessionStore!: ReturnType<typeof useDocumentSessionStore>;
    const flushPendingCellChanges = vi.fn().mockImplementation(async () => {
      documentSessionStore.openDocumentResponse(openedResponse(2, "next.xlsx"), "/tmp/next.xlsx");
      return true;
    });
    const setup = setupCommands(flushPendingCellChanges);
    documentSessionStore = setup.documentSessionStore;

    await setup.commands.handleSearch("A1", "allSheets");

    expect(api.search).not.toHaveBeenCalled();
    expect(documentSessionStore.documentId).toBe('2');
  });

  it("switches to a newly added sheet only after the mutation is applied", async () => {
    const api = await import("@/api");
    const addSheet = deferred<EditorMutationResponse>();
    vi.mocked(api.addSheet).mockReturnValue(addSheet.promise);
    const setup = setupCommands();

    const command = setup.commands.handleAddSheet();
    await Promise.resolve();

    expect(setup.currentSheetIndex.value).toBe(0);

    addSheet.resolve(mutationResponse());
    await command;

    expect(setup.applyMutationResponse).toHaveBeenCalledTimes(1);
    expect(setup.currentSheetIndex.value).toBe(2);
  });

  it("does not run post-apply sheet selection when a mutation response is ignored", async () => {
    const api = await import("@/api");
    const setup = setupCommands();
    vi.mocked(api.addSheet).mockResolvedValue(mutationResponse({ documentId: '2' }));
    setup.applyMutationResponse.mockResolvedValue({
      data: setup.documentSessionStore.data,
      resyncRequired: false,
      applied: false,
    });

    await setup.commands.handleAddSheet();

    expect(setup.applyMutationResponse).toHaveBeenCalledTimes(1);
    expect(setup.currentSheetIndex.value).toBe(0);
  });

  it("uses the latest same-document revision after flushing pending edits", async () => {
    const api = await import("@/api");
    let documentSessionStore!: ReturnType<typeof useDocumentSessionStore>;
    const flushPendingCellChanges = vi.fn().mockImplementation(async () => {
      documentSessionStore.revision = '4';
      return true;
    });
    const setup = setupCommands(flushPendingCellChanges);
    documentSessionStore = setup.documentSessionStore;

    await setup.commands.handleAddRow();

    expect(api.addRow).toHaveBeenCalledWith(
      { documentId: '1', baseRevision: '4' },
      0,
      1
    );
  });

  it("does not report a mutation failure when stale projection recovery succeeds", async () => {
    const api = await import("@/api");
    const elementPlus = await import("element-plus");
    const setup = setupCommands();
    const fresh: FileData = {
      path: "/tmp/book.xlsx",
      fileName: "book.xlsx",
      sheets: [sheet("Sheet1", [[text("fresh")]])],
    };
    vi.mocked(api.addRow).mockResolvedValue(mutationResponse({ revision: '3' }));
    vi.mocked(api.getEditorState).mockResolvedValue({
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
    });
    vi.mocked(api.getCurrentFileData).mockResolvedValue(fresh);
    setup.applyMutationResponse.mockRejectedValue(new Error("projection unavailable"));

    await setup.commands.handleAddRow();

    expect(api.getCurrentFileData).toHaveBeenCalledWith({ documentId: '1', baseRevision: '3' });
    expect(api.getEditorState).toHaveBeenCalledWith({ documentId: '1', baseRevision: '3' });
    expect(setup.documentSessionStore.data?.sheets[0].rows[0][0]).toEqual(text("fresh"));
    expect(setup.documentSessionStore.projectionStale).toBe(false);
    expect(elementPlus.ElMessage.error).not.toHaveBeenCalled();
  });

  it("reports refresh failure separately when a mutation was already applied", async () => {
    const api = await import("@/api");
    const elementPlus = await import("element-plus");
    const setup = setupCommands();
    vi.mocked(api.addRow).mockResolvedValue(mutationResponse({ revision: '3' }));
    vi.mocked(api.getEditorState).mockRejectedValue(new Error("state unavailable"));
    vi.mocked(api.getCurrentFileData).mockRejectedValue(new Error("projection unavailable"));
    setup.applyMutationResponse.mockRejectedValue(new Error("projection unavailable"));

    await setup.commands.handleAddRow();

    expect(api.addRow).toHaveBeenCalled();
    expect(api.getCurrentFileData).toHaveBeenCalledWith({ documentId: '1', baseRevision: '3' });
    expect(setup.documentSessionStore.projectionStale).toBe(true);
    expect(elementPlus.ElMessage.error).toHaveBeenCalledWith(
      "Change was applied, but the editor could not refresh: Error: projection unavailable"
    );
  });

  it("does not mark projection stale when only the post-apply UI callback fails", async () => {
    const api = await import("@/api");
    const elementPlus = await import("element-plus");
    const setup = setupCommands();
    vi.spyOn(setup.selectionStore, "activateSheet")
      .mockImplementation(() => {
        throw new Error("sheet switch failed");
      });
    vi.mocked(api.addSheet).mockResolvedValue(mutationResponse({ revision: '1' }));
    setup.applyMutationResponse.mockImplementation(async (response) => {
      return setup.documentSessionStore.applyMutationResponse(response);
    });

    await setup.commands.handleAddSheet();

    expect(setup.applyMutationResponse).toHaveBeenCalledTimes(1);
    expect(api.getCurrentFileData).not.toHaveBeenCalled();
    expect(api.getEditorState).not.toHaveBeenCalled();
    expect(setup.documentSessionStore.projectionStale).toBe(false);
    expect(elementPlus.ElMessage.error).toHaveBeenCalledWith(
      "Change was applied, but the editor UI could not update: Error: sheet switch failed"
    );
    expect(elementPlus.ElMessage.error).not.toHaveBeenCalledWith(
      expect.stringContaining("Failed to add sheet")
    );
  });
});
