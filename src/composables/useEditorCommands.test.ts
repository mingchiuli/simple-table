import { beforeEach, describe, expect, it, vi } from "vitest";
import { computed, type Ref } from "vue";
import { createPinia, setActivePinia, storeToRefs } from "pinia";
import { useEditorCommands } from "@/composables/useEditorCommands";
import { useDocumentSessionStore } from "@/stores/documentSession";
import { useEditorSelectionStore } from "@/stores/editorSelection";
import { useDocumentSessionCoordinator } from "@/application/documentSessionCoordinator";
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
import { openResponseFromFileData } from "@/test/documentFixtures";
import { sheetCell } from "@/stores/documentProjection";

vi.mock("element-plus", () => ({
  ElMessage: {
    error: vi.fn(),
    warning: vi.fn(),
  },
}));

vi.mock("@/api", () => ({
  addRow: vi.fn(),
  addSheet: vi.fn(),
  getCurrentDocumentProjection: vi.fn(),
  getSheetRegionProjection: vi.fn(),
  getEditorState: vi.fn(),
  getActiveDocument: vi.fn(),
  getMutationResult: vi.fn().mockResolvedValue({ status: 'missing' }),
  search: vi.fn().mockResolvedValue({ results: [], truncated: false }),
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
  const editorSession = {
      documentId: String(documentId) as `${bigint}`,
      revision: '0' as const,
      formulaStatus: readyFormulaStatus(),
      capabilities: defaultWorkbookCapabilities(),
      editorState: {
        canUndo: true,
        canRedo: true,
        isDirty: false,
        history: defaultHistoryStatus(),
      },
    };
  return openResponseFromFileData(fileData, editorSession);
}

function mutationResponse(partial: Partial<EditorMutationResponse> = {}): EditorMutationResponse {
  return {
    protocolVersion: 4,
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
  const documentSessionCoordinator = useDocumentSessionCoordinator();
  documentSessionCoordinator.openDocumentResponse(openedResponse(), "/tmp/book.xlsx");
  const selectionStore = useEditorSelectionStore();
  const { currentSheetIndex, selectedCell } = storeToRefs(selectionStore);
  const activeSheetIndex = overrides.currentSheetIndex ?? currentSheetIndex;
  const fileData = computed(() => documentSessionStore.data);
  const currentSheet = computed(() => documentSessionStore.loadedSheet(activeSheetIndex.value));
  const applyMutationResponse = vi.spyOn(
    documentSessionCoordinator,
    "applyMutationResponseWithResync"
  ).mockImplementation(async (_response: EditorMutationResponse) => ({
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
  });

  return {
    commands,
    documentSessionStore,
    documentSessionCoordinator,
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
      expect.objectContaining({
        documentId: '1', baseRevision: '4', commandId: expect.any(String),
      }),
      0,
      1
    );
  });

  it("appends rows at the manifest extent instead of the loaded tile boundary", async () => {
    const api = await import("@/api");
    const setup = setupCommands();
    const data = setup.documentSessionStore.data;
    if (!data) throw new Error("document setup failed");
    setup.documentSessionStore.data = {
      ...data,
      sheets: data.sheets.map((slot, index) =>
        index === 0 ? { ...slot, extent: { rowCount: 10_000, columnCount: 1 } } : slot
      ),
    };

    await setup.commands.handleAddRow();

    expect(api.addRow).toHaveBeenCalledWith(
      expect.objectContaining({
        documentId: '1', baseRevision: '0', commandId: expect.any(String),
      }),
      0,
      10_000
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
    vi.mocked(api.getCurrentDocumentProjection).mockResolvedValue(
      openResponseFromFileData(fresh, {
        ...openedResponse().editorSession,
        revision: '3',
      })
    );
    setup.applyMutationResponse.mockRejectedValue(new Error("projection unavailable"));

    await setup.commands.handleAddRow();

    expect(api.getCurrentDocumentProjection).toHaveBeenCalledWith(
      { documentId: '1', baseRevision: '3' },
      0
    );
    expect(api.getEditorState).toHaveBeenCalledWith({ documentId: '1', baseRevision: '3' });
    expect(sheetCell(setup.documentSessionStore.data?.sheets[0], 0, 0)).toEqual(text("fresh"));
    expect(setup.documentSessionStore.projectionStale).toBe(false);
    expect(elementPlus.ElMessage.error).not.toHaveBeenCalled();
  });

  it("reports refresh failure separately when a mutation was already applied", async () => {
    const api = await import("@/api");
    const elementPlus = await import("element-plus");
    const setup = setupCommands();
    vi.mocked(api.addRow).mockResolvedValue(mutationResponse({ revision: '3' }));
    vi.mocked(api.getEditorState).mockRejectedValue(new Error("state unavailable"));
    vi.mocked(api.getCurrentDocumentProjection)
      .mockRejectedValue(new Error("projection unavailable"));
    setup.applyMutationResponse.mockRejectedValue(new Error("projection unavailable"));

    await setup.commands.handleAddRow();

    expect(api.addRow).toHaveBeenCalled();
    expect(api.getCurrentDocumentProjection).toHaveBeenCalledWith(
      { documentId: '1', baseRevision: '3' },
      0
    );
    expect(setup.documentSessionStore.projectionStale).toBe(true);
    expect(elementPlus.ElMessage.error).toHaveBeenCalledWith(
      "Change was applied, but the editor could not refresh: Error: projection unavailable"
    );
  });

  it("recovers an applied mutation when both command responses are lost", async () => {
    const api = await import("@/api");
    const elementPlus = await import("element-plus");
    const setup = setupCommands();
    const recovered = openedResponse();
    recovered.editorSession.revision = '1';
    recovered.document.sheets[0].name = 'Recovered';
    vi.mocked(api.addRow).mockRejectedValue(new Error("response channel closed"));
    vi.mocked(api.getActiveDocument).mockResolvedValue(recovered);
    vi.mocked(api.getCurrentDocumentProjection).mockResolvedValue(recovered);

    await setup.commands.handleAddRow();

    expect(api.addRow).toHaveBeenCalledTimes(2);
    const firstContext = vi.mocked(api.addRow).mock.calls[0][0];
    const retryContext = vi.mocked(api.addRow).mock.calls[1][0];
    expect(retryContext.commandId).toBe(firstContext.commandId);
    expect(setup.documentSessionStore.revision).toBe('1');
    expect(setup.documentSessionStore.data?.sheets[0].name).toBe('Recovered');
    expect(setup.documentSessionStore.projectionStale).toBe(false);
    expect(elementPlus.ElMessage.error).not.toHaveBeenCalled();
  });

  it("recovers an ambiguous mutation from the command replay endpoint", async () => {
    const api = await import("@/api");
    const setup = setupCommands();
    const replay = mutationResponse({ revision: '1' });
    vi.mocked(api.addRow).mockRejectedValue(new Error("response channel closed"));
    vi.mocked(api.getMutationResult).mockResolvedValue({
      status: 'completed',
      response: replay,
    });

    await setup.commands.handleAddRow();

    expect(api.addRow).toHaveBeenCalledTimes(2);
    expect(api.getMutationResult).toHaveBeenCalledWith('1', expect.any(String));
    expect(setup.applyMutationResponse).toHaveBeenCalledWith(replay, expect.any(Function), 0);
    expect(api.getActiveDocument).not.toHaveBeenCalled();
  });

  it("polls a pending mutation result without blocking the command endpoint", async () => {
    vi.useFakeTimers();
    try {
      const api = await import("@/api");
      const setup = setupCommands();
      const replay = mutationResponse({ revision: '1' });
      vi.mocked(api.addRow).mockRejectedValue(new Error("response channel closed"));
      vi.mocked(api.getMutationResult)
        .mockResolvedValueOnce({ status: 'pending' })
        .mockResolvedValueOnce({ status: 'completed', response: replay });

      const command = setup.commands.handleAddRow();
      await vi.advanceTimersByTimeAsync(25);
      await command;

      expect(api.getMutationResult).toHaveBeenCalledTimes(2);
      expect(setup.applyMutationResponse).toHaveBeenCalledWith(replay, expect.any(Function), 0);
      expect(api.getActiveDocument).not.toHaveBeenCalled();
    } finally {
      vi.useRealTimers();
    }
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
    expect(api.getCurrentDocumentProjection).not.toHaveBeenCalled();
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
