import type { ComputedRef, Ref } from "vue";
import { ElMessage } from "element-plus";
import { useDocumentCommandBus } from "@/composables/useDocumentCommandBus";
import {
  useDocumentSessionStore,
} from "@/stores/documentSession";
import { useDocumentStatusStore } from "@/stores/documentStatus";
import { useEditorSelectionStore } from "@/stores/editorSelection";
import { useSearchSessionCoordinator } from '@/composables/useSearchSessionCoordinator';
import type {
  DocumentProjection,
  SearchResult,
  SearchScope,
  LoadedSheetSlot,
} from "@/types";
import { workbookSheetCapabilities } from "@/types";
import { appErrorMessage } from "@/utils/appError";

type UseEditorCommandsOptions = {
  fileData: ComputedRef<DocumentProjection | null>;
  currentSheet: ComputedRef<LoadedSheetSlot | null>;
  currentSheetIndex: Ref<number>;
  selectedCell: Ref<{ row: number; col: number } | null>;
  flushPendingCellChanges: () => Promise<boolean>;
  editorValueForCell: (sheetIndex: number, row: number, col: number) => string;
};

export function useEditorCommands({
  fileData,
  currentSheet,
  currentSheetIndex,
  selectedCell,
  flushPendingCellChanges,
  editorValueForCell,
}: UseEditorCommandsOptions) {
  const documentSessionStore = useDocumentSessionStore();
  const documentStatusStore = useDocumentStatusStore();
  const editorSelectionStore = useEditorSelectionStore();
  const searchSessionCoordinator = useSearchSessionCoordinator();
  const commandBus = useDocumentCommandBus();

  function mutationOptions(
    errorMessage: string,
    options: {
      refreshProjectionOnError?: boolean;
      afterApplied?: () => void;
    } = {}
  ) {
    return {
      flushPendingChanges: flushPendingCellChanges,
      errorMessage,
      ...options,
    };
  }

  async function handleAddRow() {
    if (isEditorCommandBlocked() || !currentSheet.value || !ensureStructureEditingAllowed("rows")) return;
    const sheetIndex = currentSheetIndex.value;
    const rowIndex = currentSheet.value.extent.rowCount;
    await commandBus.addRow(
      sheetIndex,
      rowIndex,
      mutationOptions("Failed to add row"),
    );
  }

  async function handleDeleteRow(index: number) {
    if (isEditorCommandBlocked() || !currentSheet.value || !ensureStructureEditingAllowed("rows")) return;
    const sheetIndex = currentSheetIndex.value;
    await commandBus.deleteRow(
      sheetIndex,
      index,
      mutationOptions("Failed to delete row"),
    );
  }

  async function handleAddColumn() {
    if (isEditorCommandBlocked() || !currentSheet.value || !ensureStructureEditingAllowed("columns")) return;
    const sheetIndex = currentSheetIndex.value;
    const colIndex = selectedCell.value?.col ?? currentSheet.value.extent.columnCount;
    await commandBus.addColumn(
      sheetIndex,
      colIndex,
      mutationOptions("Failed to add column"),
    );
  }

  async function handleDeleteColumn(index: number) {
    if (isEditorCommandBlocked() || !currentSheet.value || !ensureStructureEditingAllowed("columns")) return;
    const sheetIndex = currentSheetIndex.value;
    await commandBus.deleteColumn(
      sheetIndex,
      index,
      mutationOptions("Failed to delete column"),
    );
  }

  async function handleAddSheet() {
    if (isEditorCommandBlocked() || !fileData.value || !ensureStructureEditingAllowed("sheets")) return;
    const newSheetIndex = fileData.value.sheets.length;
    await commandBus.addSheet(
      mutationOptions("Failed to add sheet", {
        afterApplied: () => {
          editorSelectionStore.activateSheet(newSheetIndex);
        },
      }),
    );
  }

  async function handleDeleteSheet() {
    if (isEditorCommandBlocked() || !ensureStructureEditingAllowed("sheets")) return;
    if (!fileData.value || fileData.value.sheets.length <= 1) {
      ElMessage.warning("Cannot delete the last sheet");
      return;
    }

    const deletedIndex = currentSheetIndex.value;
    await commandBus.deleteSheet(
      deletedIndex,
      mutationOptions("Failed to delete sheet"),
    );
  }

  async function handleSheetChange(index: number) {
    if (isEditorCommandBlocked()) return;
    if (!(await commandBus.ensureSheetLoaded(index, flushPendingCellChanges))) return;
    editorSelectionStore.switchSheet(index, (cell) =>
      editorValueForCell(index, cell.row, cell.col)
    );
  }

  async function handleUndo() {
    if (isEditorCommandBlocked() || !documentStatusStore.canUndo) return;
    await commandBus.undo(mutationOptions("Failed to undo"));
  }

  async function handleRedo() {
    if (isEditorCommandBlocked() || !documentStatusStore.canRedo) return;
    await commandBus.redo(mutationOptions("Failed to redo"));
  }

  async function handleSearch(query: string, scope: SearchScope) {
    if (!fileData.value || isEditorCommandBlocked()) return;
    const requestId = searchSessionCoordinator.beginSearch(query);
    try {
      const outcome = await commandBus.search(
        query,
        scope,
        currentSheetIndex.value,
        flushPendingCellChanges,
      );
      if (outcome) {
        searchSessionCoordinator.applySearchOutcome(requestId, outcome);
      }
    } catch (error) {
      ElMessage.error(`Search failed: ${appErrorMessage(error)}`);
    } finally {
      searchSessionCoordinator.finishSearch(requestId);
    }
  }

  async function handleSearchResultClick(result: SearchResult) {
    if (isEditorCommandBlocked()) return;
    if (!(await commandBus.ensureSheetLoaded(result.sheetIndex, flushPendingCellChanges))) return;
    if (!(await commandBus.ensureSheetRegionLoaded({
      sheetIndex: result.sheetIndex,
      rowStart: result.row,
      rowEnd: result.row + 1,
      colStart: result.col,
      colEnd: result.col + 1,
    }))) return;
    editorSelectionStore.focusSearchResult(
      result.sheetIndex,
      result.row,
      result.col,
      editorValueForCell(result.sheetIndex, result.row, result.col)
    );
  }

  function handleClearSearch() {
    searchSessionCoordinator.clearSearch();
  }

  function handleSelectCell(row: number, col: number) {
    if (isEditorCommandBlocked()) return;
    editorSelectionStore.selectCell(row, col, false);
  }

  async function handleColumnResize(colIndex: number, width: number) {
    if (!fileData.value || isEditorCommandBlocked() || !ensureResizeAllowed()) return;
    const sheetIndex = currentSheetIndex.value;
    await commandBus.setColumnWidth(
      sheetIndex,
      colIndex,
      width,
      mutationOptions("Failed to resize column", { refreshProjectionOnError: true }),
    );
  }

  async function handleRowResize(rowIndex: number, height: number) {
    if (!fileData.value || isEditorCommandBlocked() || !ensureResizeAllowed()) return;
    const sheetIndex = currentSheetIndex.value;
    await commandBus.setRowHeight(
      sheetIndex,
      rowIndex,
      height,
      mutationOptions("Failed to resize row", { refreshProjectionOnError: true }),
    );
  }

  function ensureStructureEditingAllowed(kind: "rows" | "columns" | "sheets"): boolean {
    const capabilities = documentStatusStore.capabilities;
    const sheetCapabilities = currentSheetCapabilities();
    const allowed = kind === "rows"
      ? sheetCapabilities.canInsertDeleteRows
      : kind === "columns"
        ? sheetCapabilities.canInsertDeleteColumns
        : capabilities.structure.canInsertDeleteSheets;
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
    const sheetCapabilities = currentSheetCapabilities();
    if (kind === "rows") {
      return sheetCapabilities.blockedRowStructureReasons
        ?? capabilities.structure.blockedStructureReasons
        ?? [];
    }
    if (kind === "columns") {
      return sheetCapabilities.blockedColumnStructureReasons
        ?? capabilities.structure.blockedStructureReasons
        ?? [];
    }
    return capabilities.structure.blockedSheetStructureReasons
      ?? capabilities.structure.blockedStructureReasons
      ?? [];
  }

  function ensureResizeAllowed(): boolean {
    const sheetCapabilities = currentSheetCapabilities();
    if (sheetCapabilities.canResizeRowsColumns) return true;
    const reason = sheetCapabilities.blockedResizeReasons?.join(", ");
    ElMessage.warning(
      reason
        ? `Row and column resizing is disabled for this workbook: ${reason}`
        : "Row and column resizing is disabled for this workbook"
    );
    return false;
  }

  function currentSheetCapabilities() {
    return workbookSheetCapabilities(documentStatusStore.capabilities, currentSheetIndex.value);
  }

  function isEditorCommandBlocked(): boolean {
    return documentSessionStore.isEditorInteractionLocked;
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
