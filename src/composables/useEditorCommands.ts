import type { ComputedRef, Ref } from "vue";
import * as api from "@/api";
import { useDocumentSessionStore } from "@/stores/documentSession";
import { useSearchSessionStore } from "@/stores/searchSession";
import { useSheetLayoutStore } from "@/stores/sheetLayout";
import type { EditorMutationResponse, FileData, SearchResult, SheetData } from "@/types";

type UseEditorCommandsOptions = {
  fileData: ComputedRef<FileData | null>;
  currentSheet: ComputedRef<SheetData | null>;
  currentSheetIndex: Ref<number>;
  cellEditorValue: Ref<string>;
  isLoading: Ref<boolean>;
  flushPendingCellChanges: () => Promise<boolean>;
  editorValueForCell: (sheetIndex: number, row: number, col: number) => string;
  applyMutationResponse: (response: EditorMutationResponse) => void;
};

export function useEditorCommands({
  fileData,
  currentSheet,
  currentSheetIndex,
  cellEditorValue,
  isLoading,
  flushPendingCellChanges,
  editorValueForCell,
  applyMutationResponse,
}: UseEditorCommandsOptions) {
  const documentSessionStore = useDocumentSessionStore();
  const searchSessionStore = useSearchSessionStore();
  const sheetLayoutStore = useSheetLayoutStore();

  async function runEditorCommand(
    action: () => Promise<EditorMutationResponse>,
    message: string
  ) {
    try {
      isLoading.value = true;
      if (!(await flushPendingCellChanges())) return;
      applyMutationResponse(await action());
    } catch (error) {
      ElMessage.error(`${message}: ${error}`);
    } finally {
      isLoading.value = false;
    }
  }

  async function handleAddRow() {
    if (!currentSheet.value) return;
    const rowIndex = currentSheet.value.rows.length;
    await runEditorCommand(
      () => api.addRow(currentSheetIndex.value, rowIndex),
      "Failed to add row"
    );
  }

  async function handleDeleteRow(index: number) {
    if (!currentSheet.value) return;
    await runEditorCommand(
      () => api.deleteRow(currentSheetIndex.value, index),
      "Failed to delete row"
    );
  }

  async function handleAddColumn() {
    if (!currentSheet.value) return;
    await runEditorCommand(
      () => api.addColumn(currentSheetIndex.value),
      "Failed to add column"
    );
  }

  async function handleDeleteColumn(index: number) {
    if (!currentSheet.value) return;
    await runEditorCommand(
      () => api.deleteColumn(currentSheetIndex.value, index),
      "Failed to delete column"
    );
  }

  async function handleAddSheet() {
    if (!fileData.value) return;
    const newSheetIndex = fileData.value.sheets.length;
    await runEditorCommand(async () => {
      const response = await api.addSheet();
      documentSessionStore.clearSelection();
      currentSheetIndex.value = newSheetIndex;
      return response;
    }, "Failed to add sheet");
  }

  async function handleDeleteSheet() {
    if (!fileData.value || fileData.value.sheets.length <= 1) {
      ElMessage.warning("Cannot delete the last sheet");
      return;
    }

    const deletedIndex = currentSheetIndex.value;
    const nextSheetIndex = deletedIndex > 0 ? deletedIndex - 1 : 0;
    await runEditorCommand(async () => {
      const response = await api.deleteSheet(deletedIndex);
      currentSheetIndex.value = nextSheetIndex;
      return response;
    }, "Failed to delete sheet");
  }

  function handleSheetChange(index: number) {
    documentSessionStore.rememberCurrentSheetSelection();
    cellEditorValue.value = "";
    documentSessionStore.restoreSheetSelection(index, (cell) =>
      editorValueForCell(index, cell.row, cell.col)
    );
  }

  async function handleUndo() {
    if (!documentSessionStore.canUndo) return;
    await runEditorCommand(() => api.undo(), "Failed to undo");
  }

  async function handleRedo() {
    if (!documentSessionStore.canRedo) return;
    await runEditorCommand(() => api.redo(), "Failed to redo");
  }

  async function handleSearch(query: string, scope: "currentSheet" | "allSheets") {
    if (!fileData.value) return;

    searchSessionStore.searchQuery = query;
    try {
      searchSessionStore.isSearching = true;
      if (!(await flushPendingCellChanges())) return;

      searchSessionStore.searchResults = await api.search(
        query,
        scope,
        scope === "currentSheet" ? currentSheetIndex.value : null
      );
    } catch (error) {
      ElMessage.error(`Search failed: ${error}`);
    } finally {
      searchSessionStore.isSearching = false;
    }
  }

  function handleSearchResultClick(result: SearchResult) {
    if (result.sheetIndex !== currentSheetIndex.value) {
      currentSheetIndex.value = result.sheetIndex;
    }
    documentSessionStore.selectCell(result.row, result.col, true);
    cellEditorValue.value = editorValueForCell(result.sheetIndex, result.row, result.col);
  }

  function handleClearSearch() {
    searchSessionStore.clearSearch();
  }

  function handleSelectCell(row: number, col: number) {
    documentSessionStore.selectCell(row, col, false);
  }

  async function handleColumnResize(colIndex: number, width: number) {
    if (!fileData.value) return;
    const sheetIndex = currentSheetIndex.value;
    const oldWidth = sheetLayoutStore.sheetColumnWidths[sheetIndex]?.[colIndex];
    try {
      isLoading.value = true;
      if (!(await flushPendingCellChanges())) return;
      sheetLayoutStore.setColumnWidth(sheetIndex, colIndex, width);
      applyMutationResponse(await api.setColumnWidth(sheetIndex, colIndex, width));
    } catch (error) {
      sheetLayoutStore.setColumnWidth(sheetIndex, colIndex, oldWidth);
      ElMessage.error(`Failed to resize column: ${error}`);
    } finally {
      isLoading.value = false;
    }
  }

  async function handleRowResize(rowIndex: number, height: number) {
    if (!fileData.value) return;
    const sheetIndex = currentSheetIndex.value;
    const oldHeight = sheetLayoutStore.sheetRowHeights[sheetIndex]?.[rowIndex];
    try {
      isLoading.value = true;
      if (!(await flushPendingCellChanges())) return;
      sheetLayoutStore.setRowHeight(sheetIndex, rowIndex, height);
      applyMutationResponse(await api.setRowHeight(sheetIndex, rowIndex, height));
    } catch (error) {
      sheetLayoutStore.setRowHeight(sheetIndex, rowIndex, oldHeight);
      ElMessage.error(`Failed to resize row: ${error}`);
    } finally {
      isLoading.value = false;
    }
  }

  return {
    handleAddRow,
    handleDeleteRow,
    handleAddColumn,
    handleDeleteColumn,
    handleAddSheet,
    handleDeleteSheet,
    handleSheetChange,
    handleUndo,
    handleRedo,
    handleSearch,
    handleSearchResultClick,
    handleClearSearch,
    handleSelectCell,
    handleColumnResize,
    handleRowResize,
  };
}
