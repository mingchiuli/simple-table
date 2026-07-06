import type { ComputedRef, Ref } from "vue";
import * as api from "@/api";
import { useDocumentSessionStore } from "@/stores/documentSession";
import { useDocumentStatusStore } from "@/stores/documentStatus";
import { useSearchSessionStore } from "@/stores/searchSession";
import type { EditorMutationResponse, FileData, SearchResult, SheetData } from "@/types";
import { enqueueEditorMutation, waitForEditorMutations } from "@/composables/useEditorMutationQueue";
import { calculateSheetExtent } from "@/table-geometry/sheetExtent";

type UseEditorCommandsOptions = {
  fileData: ComputedRef<FileData | null>;
  currentSheet: ComputedRef<SheetData | null>;
  currentSheetIndex: Ref<number>;
  selectedCell: Ref<{ row: number; col: number } | null>;
  cellEditorValue: Ref<string>;
  isLoading: Ref<boolean>;
  flushPendingCellChanges: () => Promise<boolean>;
  editorValueForCell: (sheetIndex: number, row: number, col: number) => string;
  applyMutationResponse: (response: EditorMutationResponse) => Promise<void>;
  refreshProjectionFromBackend: () => Promise<void>;
};

export function useEditorCommands({
  fileData,
  currentSheet,
  currentSheetIndex,
  selectedCell,
  cellEditorValue,
  isLoading,
  flushPendingCellChanges,
  editorValueForCell,
  applyMutationResponse,
  refreshProjectionFromBackend,
}: UseEditorCommandsOptions) {
  const documentSessionStore = useDocumentSessionStore();
  const documentStatusStore = useDocumentStatusStore();
  const searchSessionStore = useSearchSessionStore();

  async function runEditorCommand(
    action: () => Promise<EditorMutationResponse>,
    message: string
  ) {
    try {
      isLoading.value = true;
      if (!(await flushPendingCellChanges())) return;
      await enqueueEditorMutation(documentSessionStore.mutationScope, async () => {
        await applyMutationResponse(await action());
      });
    } catch (error) {
      await refreshAfterMutationError();
      ElMessage.error(`${message}: ${error}`);
    } finally {
      isLoading.value = false;
    }
  }

  async function handleAddRow() {
    if (!currentSheet.value || !ensureStructureEditingAllowed("rows")) return;
    const rowIndex = sheetExtent(currentSheet.value).rowCount;
    await runEditorCommand(
      () => api.addRow(currentSheetIndex.value, rowIndex),
      "Failed to add row"
    );
  }

  async function handleDeleteRow(index: number) {
    if (!currentSheet.value || !ensureStructureEditingAllowed("rows")) return;
    await runEditorCommand(
      () => api.deleteRow(currentSheetIndex.value, index),
      "Failed to delete row"
    );
  }

  async function handleAddColumn() {
    if (!currentSheet.value || !ensureStructureEditingAllowed("columns")) return;
    const colIndex = selectedCell.value?.col ?? sheetExtent(currentSheet.value).columnCount;
    await runEditorCommand(
      () => api.addColumn(currentSheetIndex.value, colIndex),
      "Failed to add column"
    );
  }

  async function handleDeleteColumn(index: number) {
    if (!currentSheet.value || !ensureStructureEditingAllowed("columns")) return;
    await runEditorCommand(
      () => api.deleteColumn(currentSheetIndex.value, index),
      "Failed to delete column"
    );
  }

  async function handleAddSheet() {
    if (!fileData.value || !ensureStructureEditingAllowed("sheets")) return;
    const newSheetIndex = fileData.value.sheets.length;
    await runEditorCommand(async () => {
      const response = await api.addSheet();
      documentSessionStore.clearSelection();
      currentSheetIndex.value = newSheetIndex;
      return response;
    }, "Failed to add sheet");
  }

  async function handleDeleteSheet() {
    if (!ensureStructureEditingAllowed("sheets")) return;
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
    if (!documentStatusStore.canUndo) return;
    await runEditorCommand(() => api.undo(), "Failed to undo");
  }

  async function handleRedo() {
    if (!documentStatusStore.canRedo) return;
    await runEditorCommand(() => api.redo(), "Failed to redo");
  }

  async function handleSearch(query: string, scope: "currentSheet" | "allSheets") {
    if (!fileData.value) return;

    searchSessionStore.searchQuery = query;
    try {
      searchSessionStore.isSearching = true;
      if (!(await flushPendingCellChanges())) return;
      await waitForEditorMutations(documentSessionStore.mutationScope);

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
    if (!fileData.value || !ensureResizeAllowed()) return;
    const sheetIndex = currentSheetIndex.value;
    try {
      isLoading.value = true;
      if (!(await flushPendingCellChanges())) return;
      await enqueueEditorMutation(documentSessionStore.mutationScope, async () => {
        await applyMutationResponse(await api.setColumnWidth(sheetIndex, colIndex, width));
      });
    } catch (error) {
      await refreshAfterMutationError({ refreshProjection: true });
      ElMessage.error(`Failed to resize column: ${error}`);
    } finally {
      isLoading.value = false;
    }
  }

  async function handleRowResize(rowIndex: number, height: number) {
    if (!fileData.value || !ensureResizeAllowed()) return;
    const sheetIndex = currentSheetIndex.value;
    try {
      isLoading.value = true;
      if (!(await flushPendingCellChanges())) return;
      await enqueueEditorMutation(documentSessionStore.mutationScope, async () => {
        await applyMutationResponse(await api.setRowHeight(sheetIndex, rowIndex, height));
      });
    } catch (error) {
      await refreshAfterMutationError({ refreshProjection: true });
      ElMessage.error(`Failed to resize row: ${error}`);
    } finally {
      isLoading.value = false;
    }
  }

  function ensureStructureEditingAllowed(kind: "rows" | "columns" | "sheets"): boolean {
    const capabilities = documentStatusStore.capabilities;
    const allowed = kind === "rows"
      ? capabilities.canInsertDeleteRows
      : kind === "columns"
        ? capabilities.canInsertDeleteColumns
        : capabilities.canInsertDeleteSheets;
    if (allowed) return true;
    const reason = structureBlockReasons(kind).join(", ");
    ElMessage.warning(
      reason
        ? `${structureLabel(kind)} editing is disabled for this workbook: ${reason}`
        : `${structureLabel(kind)} editing is disabled for this workbook`
    );
    return false;
  }

  function structureLabel(kind: "rows" | "columns" | "sheets"): string {
    return kind === "rows" ? "Row" : kind === "columns" ? "Column" : "Sheet";
  }

  function structureBlockReasons(kind: "rows" | "columns" | "sheets"): string[] {
    const capabilities = documentStatusStore.capabilities;
    if (kind === "rows") {
      return capabilities.blockedRowStructureReasons ?? capabilities.blockedStructureReasons ?? [];
    }
    if (kind === "columns") {
      return capabilities.blockedColumnStructureReasons ?? capabilities.blockedStructureReasons ?? [];
    }
    return capabilities.blockedSheetStructureReasons ?? capabilities.blockedStructureReasons ?? [];
  }

  function ensureResizeAllowed(): boolean {
    if (documentStatusStore.capabilities.canResizeRowsColumns) return true;
    const reason = documentStatusStore.capabilities.blockedResizeReasons?.join(", ");
    ElMessage.warning(
      reason
        ? `Row and column resizing is disabled for this workbook: ${reason}`
        : "Row and column resizing is disabled for this workbook"
    );
    return false;
  }

  async function refreshAfterMutationError(
    options: { refreshProjection?: boolean } = {}
  ) {
    try {
      documentSessionStore.applyEditorSession(await api.getEditorState());
      if (options.refreshProjection && fileData.value) {
        await refreshProjectionFromBackend();
      }
    } catch (error) {
      console.error("Failed to refresh editor state after mutation error:", error);
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

function sheetExtent(sheet: SheetData) {
  return calculateSheetExtent(
    sheet.rows,
    sheet.merges,
    sheet.columnWidths,
    sheet.rowHeights,
    sheet.rich
  );
}
