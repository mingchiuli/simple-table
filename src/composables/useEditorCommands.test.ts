import { beforeEach, describe, expect, it, vi } from "vitest";
import { computed, ref } from "vue";
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
}));

function text(value: string): CellValue {
  return { type: "cell", kind: "text", raw: value, display: value };
}

function sheet(name: string, rows: CellValue[][]): SheetData {
  return { name, rows, merges: [], rich: defaultRichProjection() };
}

function openedResponse(): OpenDocumentResponse {
  const fileData: FileData = {
    path: "/tmp/book.xlsx",
    fileName: "book.xlsx",
    sheets: [
      sheet("Sheet1", [[text("A1")]]),
      sheet("Sheet2", [[text("B1")]]),
    ],
  };
  return {
    fileData,
    editorSession: {
      documentId: 1,
      revision: 0,
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

function setupCommands(isLoading = ref(false)) {
  const documentSessionStore = useDocumentSessionStore();
  documentSessionStore.openDocumentResponse(openedResponse(), "/tmp/book.xlsx");
  const selectionStore = useEditorSelectionStore();
  const { currentSheetIndex, selectedCell, cellEditorValue } = storeToRefs(selectionStore);
  const fileData = computed(() => documentSessionStore.data);
  const currentSheet = computed(() => fileData.value?.sheets[currentSheetIndex.value] ?? null);
  const flushPendingCellChanges = vi.fn().mockResolvedValue(true);
  const applyMutationResponse = vi.fn();

  const commands = useEditorCommands({
    fileData,
    currentSheet,
    currentSheetIndex,
    selectedCell,
    cellEditorValue,
    isLoading,
    flushPendingCellChanges,
    editorValueForCell: () => "",
    applyMutationResponse,
  });

  return {
    commands,
    currentSheetIndex,
    selectedCell,
    flushPendingCellChanges,
  };
}

describe("useEditorCommands", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
  });

  it("does not start structural mutations while another command is loading", async () => {
    const api = await import("@/api");
    const { commands, flushPendingCellChanges } = setupCommands(ref(true));

    await commands.handleAddRow();

    expect(api.addRow).not.toHaveBeenCalled();
    expect(flushPendingCellChanges).not.toHaveBeenCalled();
  });

  it("does not switch sheets while another command is loading", () => {
    const { commands, currentSheetIndex } = setupCommands(ref(true));

    commands.handleSheetChange(1);

    expect(currentSheetIndex.value).toBe(0);
  });

  it("does not change cell selection while another command is loading", () => {
    const { commands, selectedCell } = setupCommands(ref(true));

    commands.handleSelectCell(0, 0);

    expect(selectedCell.value).toBeNull();
  });

  it("does not navigate to search results while another command is loading", () => {
    const { commands, currentSheetIndex, selectedCell } = setupCommands(ref(true));
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
});
