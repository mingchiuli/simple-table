import type { ComputedRef, Ref } from "vue";
import { ElMessage } from "element-plus";
import * as api from "@/api";
import {
  useDocumentSessionStore,
  type MutationApplyResult,
} from "@/stores/documentSession";
import { useDocumentStatusStore } from "@/stores/documentStatus";
import { useEditorSelectionStore } from "@/stores/editorSelection";
import { useSearchSessionStore } from "@/stores/searchSession";
import type {
  EditorCommandContext,
  EditorMutationResponse,
  FileData,
  SearchResult,
  SearchScope,
  SheetData,
} from "@/types";
import { workbookSheetCapabilities } from "@/types";
import { calculateSheetExtent } from "@/table-geometry/sheetExtent";
import { appErrorMessage } from "@/utils/appError";

type UseEditorCommandsOptions = {
  fileData: ComputedRef<FileData | null>;
  currentSheet: ComputedRef<SheetData | null>;
  currentSheetIndex: Ref<number>;
  selectedCell: Ref<{ row: number; col: number } | null>;
  flushPendingCellChanges: () => Promise<boolean>;
  editorValueForCell: (sheetIndex: number, row: number, col: number) => string;
  applyMutationResponse: (response: EditorMutationResponse) => Promise<MutationApplyResult>;
};

export function useEditorCommands({
  fileData,
  currentSheet,
  currentSheetIndex,
  selectedCell,
  flushPendingCellChanges,
  editorValueForCell,
  applyMutationResponse,
}: UseEditorCommandsOptions) {
  const documentSessionStore = useDocumentSessionStore();
  const documentStatusStore = useDocumentStatusStore();
  const editorSelectionStore = useEditorSelectionStore();
  const searchSessionStore = useSearchSessionStore();

  async function runEditorMutation(
    action: (context: EditorCommandContext) => Promise<EditorMutationResponse>,
    message: string,
    options: {
      refreshProjectionOnError?: boolean;
      afterApplied?: () => void;
    } = {}
  ) {
    const releaseEditorCommand = documentSessionStore.beginEditorCommand();
    if (!releaseEditorCommand) return;
    const initialContext = documentSessionStore.currentCommandContext();
    if (!initialContext) {
      releaseEditorCommand();
      return;
    }
    try {
      if (!(await flushPendingCellChanges())) return;
      await documentSessionStore.enqueueDocumentMutation(initialContext.documentId, async (context) => {
        const response = await action(context);
        let applied = false;
        try {
          applied = (await applyMutationResponse(response)).applied;
        } catch (error) {
          if (!documentSessionStore.markProjectionStaleFromMutationResponse(response)) {
            return;
          }
          const refreshed = await refreshAfterMutationError({ refreshProjection: true });
          if (refreshed) {
            runAfterApplied(options.afterApplied);
          } else {
            ElMessage.error(
              `Change was applied, but the editor could not refresh: ${appErrorMessage(error)}`
            );
          }
          return;
        }
        if (!applied) return;
        runAfterApplied(options.afterApplied);
      });
    } catch (error) {
      await refreshAfterMutationError({
        refreshProjection: options.refreshProjectionOnError || documentSessionStore.projectionStale,
      });
      ElMessage.error(`${message}: ${appErrorMessage(error)}`);
    } finally {
      releaseEditorCommand();
    }
  }

  function runAfterApplied(afterApplied: (() => void) | undefined) {
    try {
      afterApplied?.();
    } catch (error) {
      console.error("Post-mutation UI update failed:", error);
      ElMessage.error(
        `Change was applied, but the editor UI could not update: ${appErrorMessage(error)}`
      );
    }
  }

  async function handleAddRow() {
    if (isEditorCommandBlocked() || !currentSheet.value || !ensureStructureEditingAllowed("rows")) return;
    const sheetIndex = currentSheetIndex.value;
    const rowIndex = sheetExtent(currentSheet.value).rowCount;
    await runEditorMutation(
      (context) => api.addRow(context, sheetIndex, rowIndex),
      "Failed to add row"
    );
  }

  async function handleDeleteRow(index: number) {
    if (isEditorCommandBlocked() || !currentSheet.value || !ensureStructureEditingAllowed("rows")) return;
    const sheetIndex = currentSheetIndex.value;
    await runEditorMutation(
      (context) => api.deleteRow(context, sheetIndex, index),
      "Failed to delete row"
    );
  }

  async function handleAddColumn() {
    if (isEditorCommandBlocked() || !currentSheet.value || !ensureStructureEditingAllowed("columns")) return;
    const sheetIndex = currentSheetIndex.value;
    const colIndex = selectedCell.value?.col ?? sheetExtent(currentSheet.value).columnCount;
    await runEditorMutation(
      (context) => api.addColumn(context, sheetIndex, colIndex),
      "Failed to add column"
    );
  }

  async function handleDeleteColumn(index: number) {
    if (isEditorCommandBlocked() || !currentSheet.value || !ensureStructureEditingAllowed("columns")) return;
    const sheetIndex = currentSheetIndex.value;
    await runEditorMutation(
      (context) => api.deleteColumn(context, sheetIndex, index),
      "Failed to delete column"
    );
  }

  async function handleAddSheet() {
    if (isEditorCommandBlocked() || !fileData.value || !ensureStructureEditingAllowed("sheets")) return;
    const newSheetIndex = fileData.value.sheets.length;
    await runEditorMutation(
      (context) => api.addSheet(context),
      "Failed to add sheet",
      {
        afterApplied: () => {
          editorSelectionStore.activateSheet(newSheetIndex);
        },
      }
    );
  }

  async function handleDeleteSheet() {
    if (isEditorCommandBlocked() || !ensureStructureEditingAllowed("sheets")) return;
    if (!fileData.value || fileData.value.sheets.length <= 1) {
      ElMessage.warning("Cannot delete the last sheet");
      return;
    }

    const deletedIndex = currentSheetIndex.value;
    await runEditorMutation(
      (context) => api.deleteSheet(context, deletedIndex),
      "Failed to delete sheet"
    );
  }

  function handleSheetChange(index: number) {
    if (isEditorCommandBlocked()) return;
    editorSelectionStore.switchSheet(index, (cell) =>
      editorValueForCell(index, cell.row, cell.col)
    );
  }

  async function handleUndo() {
    if (isEditorCommandBlocked() || !documentStatusStore.canUndo) return;
    await runEditorMutation((context) => api.undo(context), "Failed to undo");
  }

  async function handleRedo() {
    if (isEditorCommandBlocked() || !documentStatusStore.canRedo) return;
    await runEditorMutation((context) => api.redo(context), "Failed to redo");
  }

  async function handleSearch(query: string, scope: SearchScope) {
    if (!fileData.value || isEditorCommandBlocked()) return;
    const initialContext = documentSessionStore.currentCommandContext();
    if (!initialContext) return;

    const requestId = searchSessionStore.beginSearch(query);
    let context: EditorCommandContext | null = null;
    try {
      if (!(await flushPendingCellChanges())) return;
      await documentSessionStore.waitForMutations();
      context = documentSessionStore.commandContextForDocument(initialContext.documentId);
      if (!context) return;

      const results = await api.search(
        context,
        query,
        scope,
        scope === "currentSheet" ? currentSheetIndex.value : null
      );
      if (documentSessionStore.matchesCommandContext(context)) {
        searchSessionStore.applySearchResults(requestId, results);
      }
    } catch (error) {
      if (context && !documentSessionStore.matchesCommandContext(context)) {
        return;
      }
      ElMessage.error(`Search failed: ${appErrorMessage(error)}`);
    } finally {
      searchSessionStore.finishSearch(requestId);
    }
  }

  function handleSearchResultClick(result: SearchResult) {
    if (isEditorCommandBlocked()) return;
    editorSelectionStore.focusSearchResult(
      result.sheetIndex,
      result.row,
      result.col,
      editorValueForCell(result.sheetIndex, result.row, result.col)
    );
  }

  function handleClearSearch() {
    searchSessionStore.clearSearch();
  }

  function handleSelectCell(row: number, col: number) {
    if (isEditorCommandBlocked()) return;
    editorSelectionStore.selectCell(row, col, false);
  }

  async function handleColumnResize(colIndex: number, width: number) {
    if (!fileData.value || isEditorCommandBlocked() || !ensureResizeAllowed()) return;
    const sheetIndex = currentSheetIndex.value;
    await runEditorMutation(
      (context) => api.setColumnWidth(context, sheetIndex, colIndex, width),
      "Failed to resize column",
      { refreshProjectionOnError: true }
    );
  }

  async function handleRowResize(rowIndex: number, height: number) {
    if (!fileData.value || isEditorCommandBlocked() || !ensureResizeAllowed()) return;
    const sheetIndex = currentSheetIndex.value;
    await runEditorMutation(
      (context) => api.setRowHeight(context, sheetIndex, rowIndex, height),
      "Failed to resize row",
      { refreshProjectionOnError: true }
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

  async function refreshAfterMutationError(
    options: { refreshProjection?: boolean } = {}
  ): Promise<boolean> {
    try {
      await documentSessionStore.refreshAfterMutationFailure(
        api.getEditorState,
        options.refreshProjection && fileData.value ? api.getCurrentFileData : undefined
      );
      return true;
    } catch (error) {
      console.error("Failed to refresh editor state after mutation error:", error);
      return false;
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
