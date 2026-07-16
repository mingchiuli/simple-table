<script setup lang="ts">
import { HomeFilled } from '@element-plus/icons-vue';
import { useDocumentStatus } from '@/composables/useDocumentStatus';
import { useEditorCommands } from '@/composables/useEditorCommands';
import { useFileActions } from '@/composables/useFileActions';
import { useCellEditController } from '@/composables/useCellEditController';
import { useDocumentCommandBus } from '@/composables/useDocumentCommandBus';
import { useDocumentSessionStore } from '@/stores/documentSession';
import { useEditorSelectionStore } from '@/stores/editorSelection';
import { useSearchSessionStore } from '@/stores/searchSession';
import { Toolbar, StatusBar } from '@/components/layout';
import TableEditor from '@/components/TableEditor.vue';
import { FormulaBar } from '@/components/cell';
import { SearchPanel } from '@/components/search';
import { getCellKey } from '@/utils/cellKey';
import { cellToEditorString } from '@/utils/cellValue';
import { colToLetter } from '@/utils/excel';
import { workbookSheetCapabilities } from '@/types';
import {
  isCellLoaded,
  loadedSheetMetadata,
  sheetCell,
} from '@/stores/documentProjection';
import { createRouteFileLoader, createRouteLeaveHandler } from '@/composables/useRouteFileLoader';
import { useApplicationExitGuard } from '@/composables/useApplicationExit';
const route = useRoute();
const documentSessionStore = useDocumentSessionStore();
const editorSelectionStore = useEditorSelectionStore();
const searchSessionStore = useSearchSessionStore();
const commandBus = useDocumentCommandBus();

const {
  currentSheetIndex,
  selectedCell,
  cellEditorValue,
  autoScroll,
} = storeToRefs(editorSelectionStore);
const {
  searchResults,
  searchQuery,
  isSearching,
} = storeToRefs(searchSessionStore);
// ========== Computed values ==========
const fileData = computed(() => documentSessionStore.data);

const currentSheet = computed(() => {
  if (!fileData.value || !fileData.value.sheets.length) return null;
  return documentSessionStore.loadedSheet(currentSheetIndex.value);
});

const currentSheetMetadata = computed(() =>
  currentSheet.value ? loadedSheetMetadata(currentSheet.value) : null
);

const currentSheetExtent = computed(() => {
  return currentSheet.value?.extent ?? { rowCount: 0, columnCount: 0 };
});

const columns = computed(() =>
  Array.from({ length: currentSheetExtent.value.columnCount }, (_, i) => colToLetter(i))
);

const sheetNames = computed(() => {
  if (!fileData.value) return [];
  return fileData.value.sheets.map((s) => s.name);
});

const {
  canUndo,
  canRedo,
  hasUnsavedChanges,
  formulaStatus,
  capabilities,
  history,
  refreshEditorState,
} = useDocumentStatus();

const isFileLoading = computed(() => documentSessionStore.lifecycle === 'loading');
const isFileActionBusy = computed(() => documentSessionStore.isInteractionLocked);
const isEditorLocked = computed(() => documentSessionStore.isEditorInteractionLocked);
const canInteractWithDocument = computed(() => !isEditorLocked.value);
const currentSheetCapabilities = computed(() =>
  workbookSheetCapabilities(capabilities.value, currentSheetIndex.value)
);
const toolbarCapabilities = computed(() => ({
  canInsertDeleteRows: currentSheetCapabilities.value.canInsertDeleteRows,
  canInsertDeleteColumns: currentSheetCapabilities.value.canInsertDeleteColumns,
  canInsertDeleteSheets: capabilities.value.structure.canInsertDeleteSheets,
}));
const canEditCells = computed(() => currentSheetCapabilities.value.canEditCells && canInteractWithDocument.value);
const canResizeRowsColumns = computed(
  () => currentSheetCapabilities.value.canResizeRowsColumns && canInteractWithDocument.value
);

const {
  draftCellValues,
  flushPendingCellChanges,
  handleCellChange,
  handleCellEditing,
  handleCellEditCancel,
  handleCellEditorSubmit,
  handleDeselectCell,
} = useCellEditController({
  fileData,
  currentSheet,
  currentSheetIndex,
  selectedCell,
  cellEditorValue,
  canEditCells,
});

const {
  loadFileFromPath,
  handleOpenFile,
  handleSaveFile,
  handleExportFile,
  closeCurrentDocument,
  handleBack,
} = useFileActions({
  fileData,
  flushPendingCellChanges,
});

useApplicationExitGuard(() => closeCurrentDocument({ waitForIdle: true }));

function routeFilePath(): string | null {
  const value = route.query.file;
  if (Array.isArray(value)) {
    return value[0] || null;
  }
  return value || null;
}

const routeFileLoader = createRouteFileLoader({
  getRouteFilePath: routeFilePath,
  getCurrentFilePath: () => documentSessionStore.currentFilePath,
  loadFileFromPath,
  refreshEditorState,
});

onBeforeRouteLeave(createRouteLeaveHandler({
  routeFileLoader,
  hasActiveDocument: () =>
    documentSessionStore.data !== null || documentSessionStore.documentId !== null,
  closeCurrentDocument,
}));

function getEditorValue(sheetIndex: number, row: number, col: number): string {
  const draftValue = draftCellValues.value.get(getCellKey(sheetIndex, row, col));
  if (draftValue !== undefined) return draftValue;
  return cellToEditorString(sheetCell(fileData.value?.sheets[sheetIndex], row, col));
}

function currentCellAt(row: number, col: number) {
  return sheetCell(fileData.value?.sheets[currentSheetIndex.value], row, col);
}

function currentCellIsLoaded(row: number, col: number) {
  return isCellLoaded(fileData.value?.sheets[currentSheetIndex.value], row, col);
}

function handleViewportChange(
  rowStart: number,
  rowEnd: number,
  colStart: number,
  colEnd: number
) {
  const extent = currentSheetExtent.value;
  void commandBus.ensureSheetRegionLoaded({
    sheetIndex: currentSheetIndex.value,
    rowStart: Math.max(0, rowStart),
    rowEnd: Math.min(extent.rowCount, rowEnd),
    colStart: Math.max(0, colStart),
    colEnd: Math.min(extent.columnCount, colEnd),
  }, { priority: 'viewport' });
}

const {
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
} = useEditorCommands({
  fileData,
  currentSheet,
  currentSheetIndex,
  selectedCell,
  flushPendingCellChanges,
  editorValueForCell: getEditorValue,
});

// ========== Lifecycle ==========
watch(() => route.query.file, () => {
  routeFileLoader.enqueue(routeFilePath());
}, {
  immediate: true,
});

</script>

<template>
  <div class="app-container">
    <Toolbar
      :file-data="fileData"
      :sheet-names="sheetNames"
      :current-sheet-index="currentSheetIndex"
      :can-undo="canUndo"
      :can-redo="canRedo"
      :capabilities="toolbarCapabilities"
      :is-busy="isFileActionBusy"
      :is-editor-locked="isEditorLocked"
      :is-searching="isSearching"
      @open-file="handleOpenFile"
      @save-file="handleSaveFile"
      @export-file="handleExportFile"
      @sheet-change="handleSheetChange"
      @add-sheet="handleAddSheet"
      @delete-sheet="handleDeleteSheet"
      @add-row="handleAddRow"
      @add-column="handleAddColumn"
      @undo="handleUndo"
      @redo="handleRedo"
      @search="handleSearch"
      @clear-search="handleClearSearch"
    />

    <main class="content">
      <div class="editor-column">
        <div v-if="isFileLoading" class="skeleton-container">
          <div class="skeleton-header">
            <el-skeleton :rows="1" animated />
          </div>
          <el-skeleton :rows="10" animated />
        </div>

        <template v-else>
          <FormulaBar
            v-if="selectedCell && fileData"
            v-model="cellEditorValue"
            :cell-position="selectedCell"
            :disabled="!canEditCells || !currentCellIsLoaded(selectedCell.row, selectedCell.col)"
            @submit="handleCellEditorSubmit"
            @close="handleDeselectCell"
          />

          <div class="table-wrapper">
            <TableEditor
              :cell-at="currentCellAt"
              :is-cell-loaded="currentCellIsLoaded"
              :columns="columns"
              :sheet-index="currentSheetIndex"
              :draft-cell-values="draftCellValues"
              :merges="currentSheetMetadata?.merges"
              :selected-cell="selectedCell"
              :auto-scroll="autoScroll"
              :can-edit-cells="canEditCells"
              :can-resize-rows-columns="canResizeRowsColumns"
              :column-widths="currentSheetMetadata?.columnWidths"
              :row-heights="currentSheetMetadata?.rowHeights"
              :rich="currentSheetMetadata?.rich"
              :extent="currentSheetExtent"
              :commit-column-resize="handleColumnResize"
              :commit-row-resize="handleRowResize"
              @cell-change="handleCellChange"
              @cell-editing="handleCellEditing"
              @cell-edit-cancel="handleCellEditCancel"
              @delete-row="handleDeleteRow"
              @delete-column="handleDeleteColumn"
              @select-cell="handleSelectCell"
              @viewport-change="handleViewportChange"
            />
          </div>
        </template>
      </div>

      <SearchPanel
        class="search-panel-host"
        :results="searchResults"
        :query="searchQuery"
        :disabled="isEditorLocked"
        @result-click="handleSearchResultClick"
        @clear="handleClearSearch"
      />
    </main>

    <StatusBar
      v-if="fileData"
      :file-name="fileData.fileName"
      :has-changes="hasUnsavedChanges"
      :formula-status="formulaStatus"
      :history-status="history"
    />

    <el-button class="back-btn" circle :disabled="isFileActionBusy" @click="handleBack">
      <el-icon><HomeFilled /></el-icon>
    </el-button>
  </div>
</template>

<style scoped>
.app-container {
  display: flex;
  flex-direction: column;
  width: 100%;
  max-width: 100vw;
  height: 100%;
  background-color: var(--el-bg-color);
  position: relative;
  overflow: hidden;
}

.content {
  flex: 1;
  overflow: hidden;
  padding: 0;
  display: flex;
  flex-direction: row;
  min-width: 0;
  min-height: 0;
}

.editor-column {
  flex: 1;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  min-width: 0;
  min-height: 0;
}

.table-wrapper {
  background: var(--el-bg-color);
  flex: 1;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  min-width: 0;
  min-height: 0;
  width: 100%;
  max-width: 100%;
}

.skeleton-container {
  padding: 20px;
  background: var(--el-bg-color);
}

.skeleton-header {
  margin-bottom: 16px;
  padding-bottom: 16px;
  border-bottom: 1px solid var(--el-border-color-light);
}

.back-btn {
  position: fixed;
  bottom: max(20px, env(safe-area-inset-bottom));
  left: max(20px, env(safe-area-inset-left));
  width: 36px;
  height: 36px;
  z-index: 100;
}

@media (max-width: 900px), (pointer: coarse) {
  .content {
    position: relative;
  }

  .search-panel-host {
    position: absolute;
    right: 8px;
    bottom: 8px;
    left: 8px;
    z-index: 80;
    max-height: min(42vh, 320px);
    border: 1px solid var(--el-border-color);
    border-radius: 8px;
    box-shadow: var(--el-box-shadow-light);
  }

  .back-btn {
    right: max(12px, env(safe-area-inset-right));
    bottom: max(12px, env(safe-area-inset-bottom));
    left: auto;
    width: 40px;
    height: 40px;
  }
}

@media (max-width: 480px) {
  .search-panel-host {
    right: 6px;
    bottom: 6px;
    left: 6px;
    max-height: 46vh;
  }
}
</style>
