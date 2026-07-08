<script setup lang="ts">
import { HomeFilled } from '@element-plus/icons-vue';
import { useDocumentStatus } from '@/composables/useDocumentStatus';
import { useEditorCommands } from '@/composables/useEditorCommands';
import { useFileActions } from '@/composables/useFileActions';
import { useCellEditController } from '@/composables/useCellEditController';
import { useDocumentSessionStore } from '@/stores/documentSession';
import { useEditorSelectionStore } from '@/stores/editorSelection';
import { usePendingCellSavesStore } from '@/stores/pendingCellSaves';
import { useSearchSessionStore } from '@/stores/searchSession';
import { Toolbar, StatusBar } from '@/components/layout';
import TableEditor from '@/components/TableEditor.vue';
import { FormulaBar } from '@/components/cell';
import { SearchPanel } from '@/components/search';
import * as api from '@/api';
import { getCellKey } from '@/utils/cellKey';
import { cellToEditorString } from '@/utils/cellValue';
import { colToLetter } from '@/utils/excel';
import { calculateSheetExtent } from '@/table-geometry/sheetExtent';
import { workbookSheetCapabilities } from '@/types';
import type { EditorMutationResponse } from '@/types';
const route = useRoute();
const documentSessionStore = useDocumentSessionStore();
const editorSelectionStore = useEditorSelectionStore();
const pendingCellSavesStore = usePendingCellSavesStore();
const searchSessionStore = useSearchSessionStore();

// ========== State refs (must be declared before composables use them) ==========
const isLoading = ref(false);
const isFileLoading = ref(false);
const layoutResetKey = ref(0);
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
const {
  draftCellValues,
} = storeToRefs(pendingCellSavesStore);

// ========== Computed values ==========
const fileData = computed(() => documentSessionStore.data);

const currentSheet = computed(() => {
  if (!fileData.value || !fileData.value.sheets.length) return null;
  return fileData.value.sheets[currentSheetIndex.value];
});

const tableData = computed(() => {
  if (!currentSheet.value) return [];
  return currentSheet.value.rows;
});

const columns = computed(() => {
  const extent = calculateSheetExtent(
    tableData.value,
    currentSheet.value?.merges ?? [],
    currentSheet.value?.columnWidths,
    currentSheet.value?.rowHeights,
    currentSheet.value?.rich
  );
  return Array.from({ length: extent.columnCount }, (_, i) => colToLetter(i));
});

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
  markPendingContentChange,
  clearPendingContentChange,
  resetDocumentStatus,
} = useDocumentStatus();

const canInteractWithDocument = computed(() => !documentSessionStore.isInteractionLocked);
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

async function applyMutationResponse(response: EditorMutationResponse) {
  const result = documentSessionStore.applyMutationResponse(response);
  if (result.resyncRequired) {
    await refreshProjectionFromBackend();
  }
  refreshSelectedEditorValue();
}

async function refreshProjectionFromBackend() {
  documentSessionStore.replaceProjection(await api.getCurrentFileData());
  layoutResetKey.value += 1;
}

const {
  flushPendingCellChanges,
  refreshSelectedEditorValue,
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
  applyMutationResponse,
  markPendingContentChange,
  clearPendingContentChange,
});

const {
  loadFileFromPath,
  handleOpenFile,
  handleSaveFile,
  handleExportFile,
  handleBack,
} = useFileActions({
  fileData,
  currentSheetIndex,
  isLoading,
  isFileLoading,
  flushPendingCellChanges,
  resetDocumentStatus,
});

let routeLoadQueue = Promise.resolve();
let lastLoadedRouteFilePath: string | null = null;

function routeFilePath(): string | null {
  const value = route.query.file;
  if (Array.isArray(value)) {
    return value[0] || null;
  }
  return value || null;
}

function enqueueRouteFileLoad(filePath: string | null) {
  routeLoadQueue = routeLoadQueue
    .catch(() => undefined)
    .then(async () => {
      if (filePath !== routeFilePath()) return;
      if (!filePath) {
        lastLoadedRouteFilePath = null;
        await refreshEditorState();
        return;
      }
      if (filePath === lastLoadedRouteFilePath && documentSessionStore.currentFilePath === filePath) {
        return;
      }
      lastLoadedRouteFilePath = filePath;
      await loadFileFromPath(filePath);
    })
    .catch((error) => {
      console.error("Failed to handle route file load:", error);
    });
}

function getEditorValue(sheetIndex: number, row: number, col: number): string {
  const draftValue = draftCellValues.value.get(getCellKey(sheetIndex, row, col));
  if (draftValue !== undefined) return draftValue;
  const sheet = fileData.value?.sheets[sheetIndex];
  return cellToEditorString(sheet?.rows[row]?.[col]);
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
  cellEditorValue,
  isLoading,
  flushPendingCellChanges,
  editorValueForCell: getEditorValue,
  applyMutationResponse,
  refreshProjectionFromBackend,
});

// ========== Lifecycle ==========
watch(() => route.query.file, () => {
  enqueueRouteFileLoad(routeFilePath());
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
            :disabled="!canEditCells"
            @submit="handleCellEditorSubmit"
            @close="handleDeselectCell"
          />

          <div class="table-wrapper">
            <TableEditor
              :data="tableData"
              :columns="columns"
              :sheet-index="currentSheetIndex"
              :draft-cell-values="draftCellValues"
              :merges="currentSheet?.merges"
              :selected-cell="selectedCell"
              :auto-scroll="autoScroll"
              :can-edit-cells="canEditCells"
              :can-resize-rows-columns="canResizeRowsColumns"
              :column-widths="currentSheet?.columnWidths"
              :row-heights="currentSheet?.rowHeights"
              :layout-reset-key="layoutResetKey"
              :rich="currentSheet?.rich"
              @cell-change="handleCellChange"
              @cell-editing="handleCellEditing"
              @cell-edit-cancel="handleCellEditCancel"
              @delete-row="handleDeleteRow"
              @delete-column="handleDeleteColumn"
              @select-cell="handleSelectCell"
              @column-resize="handleColumnResize"
              @row-resize="handleRowResize"
            />
          </div>
        </template>
      </div>

      <SearchPanel
        class="search-panel-host"
        :results="searchResults"
        :query="searchQuery"
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

    <el-button class="back-btn" circle @click="handleBack">
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
