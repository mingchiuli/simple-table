<script setup lang="ts">
import { HomeFilled } from '@element-plus/icons-vue';
import { usePlatform } from '@/composables/usePlatform';
import { useDocumentStatus } from '@/composables/useDocumentStatus';
import { useFileActions } from '@/composables/useFileActions';
import { cellToEditorString, usePendingCellSave } from '@/composables/usePendingCellSave';
import { useFileDataStore } from '@/stores/fileData';
import { Toolbar, StatusBar } from '@/components/layout';
import TableEditor from '@/components/TableEditor.vue';
import { FormulaBar } from '@/components/cell';
import { SearchPanel } from '@/components/search';
import * as api from '@/api';
import { getCellKey } from '@/utils/cellKey';
import { colToLetter } from '@/utils/excel';
import type { CellValue, EditorMutationResponse, FileData, SearchResult } from '@/types';
const route = useRoute();
const fileDataStore = useFileDataStore();
const { isMobileOrTablet } = usePlatform();

// ========== State refs (must be declared before composables use them) ==========
const isLoading = ref(false);
const isFileLoading = ref(false);
const currentSheetIndex = ref(0);
const selectedCell = ref<{ row: number; col: number } | null>(null);
const cellEditorValue = ref<string>('');
const autoScroll = ref(false);
const searchResults = ref<SearchResult[]>([]);
const searchQuery = ref('');
const isSearching = ref(false);
const sheetSelectedCells = ref<Map<number, { row: number; col: number }>>(new Map());
const sheetColumnWidths = ref<Record<number, Record<number, number>>>({});
const sheetRowHeights = ref<Record<number, Record<number, number>>>({});

// ========== Computed values ==========
const fileData = computed(() => fileDataStore.data);

const currentSheet = computed(() => {
  if (!fileData.value || !fileData.value.sheets.length) return null;
  return fileData.value.sheets[currentSheetIndex.value];
});

const tableData = computed(() => {
  if (!currentSheet.value) return [];
  return currentSheet.value.rows;
});

const columns = computed(() => {
  if (!tableData.value.length) return [];
  const maxCols = Math.max(...tableData.value.map((row) => row.length));
  return Array.from({ length: maxCols }, (_, i) => colToLetter(i));
});

const sheetNames = computed(() => {
  if (!fileData.value) return [];
  return fileData.value.sheets.map((s) => s.name);
});

const {
  canUndo,
  canRedo,
  hasUnsavedChanges,
  refreshEditorState,
  markPendingContentChange,
  clearPendingContentChange,
  applyEditorState,
  resetDocumentStatus,
  markSaved,
} = useDocumentStatus();

function applyMutationResponse(response: EditorMutationResponse) {
  const nextFileData = applyMutationFileData(response);
  if (!nextFileData) return;

  applyEditorState(response.editorState);

  if (currentSheetIndex.value >= nextFileData.sheets.length) {
    currentSheetIndex.value = Math.max(0, nextFileData.sheets.length - 1);
  }

  if (selectedCell.value) {
    const sheet = nextFileData.sheets[currentSheetIndex.value];
    if (!sheet?.rows[selectedCell.value.row]?.length) {
      selectedCell.value = null;
      cellEditorValue.value = '';
      return;
    }
    if (selectedCell.value.col >= sheet.rows[selectedCell.value.row].length) {
      selectedCell.value = null;
      cellEditorValue.value = '';
      return;
    }
    cellEditorValue.value = getEditorValue(currentSheetIndex.value, selectedCell.value.row, selectedCell.value.col);
  }
}

function applyMutationFileData(response: EditorMutationResponse): FileData | null {
  if (response.kind === 'snapshot') {
    if (!response.fileData) return fileDataStore.data;
    const currentFileData = fileDataStore.data;
    const nextFileData = {
      ...response.fileData,
      path: currentFileData?.path ?? response.fileData.path,
      fileName: currentFileData?.fileName ?? response.fileData.fileName,
    };
    fileDataStore.setData(nextFileData);
    return nextFileData;
  }

  const currentFileData = fileDataStore.data;
  if (!currentFileData) return null;
  const changes = response.cellChanges ?? [];
  if (!changes.length) return currentFileData;

  const nextFileData: FileData = {
    ...currentFileData,
    sheets: [...currentFileData.sheets],
  };
  const clonedRowsBySheet = new Map<number, CellValue[][]>();

  for (const change of changes) {
    const sheet = currentFileData.sheets[change.sheetIndex];
    if (!sheet) continue;
    let rows = clonedRowsBySheet.get(change.sheetIndex);
    if (!rows) {
      rows = [...sheet.rows];
      clonedRowsBySheet.set(change.sheetIndex, rows);
      nextFileData.sheets[change.sheetIndex] = {
        ...sheet,
        rows,
      };
    }
    ensureCellExists(rows, change.row, change.col);
    rows[change.row][change.col] = change.value;
  }

  fileDataStore.setData(nextFileData);
  return nextFileData;
}

function ensureCellExists(rows: CellValue[][], row: number, col: number) {
  while (rows.length <= row) {
    rows.push([]);
  }
  rows[row] = [...rows[row]];
  while (rows[row].length <= col) {
    rows[row].push(null);
  }
}

const {
  draftCellValues,
  flushPendingCellChanges,
  handleCellChange,
  handleCellEditing,
  handleCellEditCancel,
  handleCellEditorSubmit,
  handleDeselectCell,
} = usePendingCellSave({
  fileData,
  currentSheet,
  currentSheetIndex,
  selectedCell,
  cellEditorValue,
  applyMutationResponse,
  markPendingContentChange,
  clearPendingContentChange,
});

watch(() => fileDataStore.documentVersion, () => {
  selectedCell.value = null;
  cellEditorValue.value = '';
  autoScroll.value = false;
  searchResults.value = [];
  searchQuery.value = '';
  sheetSelectedCells.value = new Map();
  sheetColumnWidths.value = {};
  sheetRowHeights.value = {};
  hydrateLayoutMapsFromFileData();
});

watch(() => fileData.value, hydrateLayoutMapsFromFileData, { immediate: true });

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
  refreshEditorState,
  markSaved,
  resetDocumentStatus,
});

function getEditorValue(sheetIndex: number, row: number, col: number): string {
  const draftValue = draftCellValues.get(getCellKey(sheetIndex, row, col));
  if (draftValue !== undefined) return draftValue;
  const sheet = fileData.value?.sheets[sheetIndex];
  return cellToEditorString(sheet?.rows[row]?.[col]);
}

// ========== Row/Column operations ==========
async function handleAddRow() {
  if (!currentSheet.value) return;
  if (!(await flushPendingCellChanges())) return;

  const newRowIndex = currentSheet.value.rows.length;

  try {
    isLoading.value = true;
    applyMutationResponse(await api.addRow(currentSheetIndex.value, newRowIndex));
  } catch (error) {
    ElMessage.error(`Failed to add row: ${error}`);
  } finally {
    isLoading.value = false;
  }
}

async function handleDeleteRow(index: number) {
  if (!currentSheet.value) return;
  if (!(await flushPendingCellChanges())) return;

  try {
    isLoading.value = true;
    applyMutationResponse(await api.deleteRow(currentSheetIndex.value, index));
  } catch (error) {
    ElMessage.error(`Failed to delete row: ${error}`);
  } finally {
    isLoading.value = false;
  }
}

async function handleAddColumn() {
  if (!currentSheet.value) return;
  if (!(await flushPendingCellChanges())) return;

  try {
    isLoading.value = true;
    applyMutationResponse(await api.addColumn(currentSheetIndex.value));
  } catch (error) {
    ElMessage.error(`Failed to add column: ${error}`);
  } finally {
    isLoading.value = false;
  }
}

async function handleDeleteColumn(index: number) {
  if (!currentSheet.value) return;
  if (!(await flushPendingCellChanges())) return;

  try {
    isLoading.value = true;
    applyMutationResponse(await api.deleteColumn(currentSheetIndex.value, index));
  } catch (error) {
    ElMessage.error(`Failed to delete column: ${error}`);
  } finally {
    isLoading.value = false;
  }
}

// ========== Sheet operations ==========
async function handleAddSheet() {
  if (!fileData.value) return;
  if (!(await flushPendingCellChanges())) return;

  const newSheetIndex = fileData.value.sheets.length;

  try {
    isLoading.value = true;
    applyMutationResponse(await api.addSheet());
    selectedCell.value = null;
    cellEditorValue.value = '';
    currentSheetIndex.value = newSheetIndex;
  } catch (error) {
    ElMessage.error(`Failed to add sheet: ${error}`);
  } finally {
    isLoading.value = false;
  }
}

async function handleDeleteSheet() {
  if (!fileData.value || fileData.value.sheets.length <= 1) {
    ElMessage.warning('Cannot delete the last sheet');
    return;
  }
  if (!(await flushPendingCellChanges())) return;

  const deletedIndex = currentSheetIndex.value;
  const newIndex = deletedIndex > 0 ? deletedIndex - 1 : 0;

  try {
    isLoading.value = true;
    applyMutationResponse(await api.deleteSheet(deletedIndex));
    currentSheetIndex.value = newIndex;
  } catch (error) {
    ElMessage.error(`Failed to delete sheet: ${error}`);
  } finally {
    isLoading.value = false;
  }
}

function handleSheetChange(index: number) {
  if (selectedCell.value !== null) {
    sheetSelectedCells.value.set(currentSheetIndex.value, selectedCell.value);
  }

  cellEditorValue.value = '';
  currentSheetIndex.value = index;

  const savedCell = sheetSelectedCells.value.get(index);
  if (savedCell) {
    selectedCell.value = savedCell;
    cellEditorValue.value = getEditorValue(index, savedCell.row, savedCell.col);
    autoScroll.value = true;
  } else {
    selectedCell.value = null;
  }
}

// ========== Undo/Redo ==========
async function handleUndo() {
  if (!canUndo.value) return;

  try {
    isLoading.value = true;
    if (!(await flushPendingCellChanges())) return;

    applyMutationResponse(await api.undo());
  } catch (error) {
    ElMessage.error(`Failed to undo: ${error}`);
  } finally {
    isLoading.value = false;
  }
}

async function handleRedo() {
  if (!canRedo.value) return;

  try {
    isLoading.value = true;
    if (!(await flushPendingCellChanges())) return;

    applyMutationResponse(await api.redo());
  } catch (error) {
    ElMessage.error(`Failed to redo: ${error}`);
  } finally {
    isLoading.value = false;
  }
}

// ========== Search ==========
async function handleSearch(query: string, scope: 'currentSheet' | 'allSheets') {
  if (!fileData.value) return;

  searchQuery.value = query;
  try {
    isSearching.value = true;
    if (!(await flushPendingCellChanges())) return;

    searchResults.value = await api.search(
      query,
      scope,
      scope === 'currentSheet' ? currentSheetIndex.value : null
    );
  } catch (error) {
    ElMessage.error(`Search failed: ${error}`);
  } finally {
    isSearching.value = false;
  }
}

function handleSearchResultClick(result: SearchResult) {
  if (result.sheetIndex !== currentSheetIndex.value) {
    currentSheetIndex.value = result.sheetIndex;
  }
  autoScroll.value = true;
  selectedCell.value = { row: result.row, col: result.col };
  cellEditorValue.value = getEditorValue(result.sheetIndex, result.row, result.col);
}

function handleClearSearch() {
  searchResults.value = [];
  searchQuery.value = '';
}

function handleSelectCell(row: number, col: number) {
  autoScroll.value = false;
  selectedCell.value = { row, col };
}

// ========== Column resize ==========
async function handleColumnResize(colIndex: number, width: number) {
  if (!fileData.value) return;
  const sheetIndex = currentSheetIndex.value;
  const oldWidth = sheetColumnWidths.value[sheetIndex]?.[colIndex];
  try {
    isLoading.value = true;
    if (!(await flushPendingCellChanges())) return;
    setLocalColumnWidth(sheetIndex, colIndex, width);
    applyMutationResponse(await api.setColumnWidth(sheetIndex, colIndex, width));
  } catch (error) {
    setLocalColumnWidth(sheetIndex, colIndex, oldWidth);
    ElMessage.error(`Failed to resize column: ${error}`);
  } finally {
    isLoading.value = false;
  }
}

async function handleRowResize(rowIndex: number, height: number) {
  if (!fileData.value) return;
  const sheetIndex = currentSheetIndex.value;
  const oldHeight = sheetRowHeights.value[sheetIndex]?.[rowIndex];
  try {
    isLoading.value = true;
    if (!(await flushPendingCellChanges())) return;
    setLocalRowHeight(sheetIndex, rowIndex, height);
    applyMutationResponse(await api.setRowHeight(sheetIndex, rowIndex, height));
  } catch (error) {
    setLocalRowHeight(sheetIndex, rowIndex, oldHeight);
    ElMessage.error(`Failed to resize row: ${error}`);
  } finally {
    isLoading.value = false;
  }
}

function setLocalColumnWidth(sheetIndex: number, colIndex: number, width: number | undefined) {
  const sheetWidths = { ...(sheetColumnWidths.value[sheetIndex] ?? {}) };
  if (width === undefined) {
    delete sheetWidths[colIndex];
  } else {
    sheetWidths[colIndex] = width;
  }
  const next = { ...sheetColumnWidths.value };
  if (Object.keys(sheetWidths).length) {
    next[sheetIndex] = sheetWidths;
  } else {
    delete next[sheetIndex];
  }
  sheetColumnWidths.value = {
    ...next,
  };
}

function setLocalRowHeight(sheetIndex: number, rowIndex: number, height: number | undefined) {
  const sheetHeights = { ...(sheetRowHeights.value[sheetIndex] ?? {}) };
  if (height === undefined) {
    delete sheetHeights[rowIndex];
  } else {
    sheetHeights[rowIndex] = height;
  }
  const next = { ...sheetRowHeights.value };
  if (Object.keys(sheetHeights).length) {
    next[sheetIndex] = sheetHeights;
  } else {
    delete next[sheetIndex];
  }
  sheetRowHeights.value = {
    ...next,
  };
}

function hydrateLayoutMapsFromFileData() {
  const data = fileData.value;
  if (!data) {
    sheetColumnWidths.value = {};
    sheetRowHeights.value = {};
    return;
  }

  sheetColumnWidths.value = Object.fromEntries(
    data.sheets
      .map((sheet, index) => [index, sheet.columnWidths ?? {}] as const)
      .filter(([, widths]) => Object.keys(widths).length > 0)
  );
  sheetRowHeights.value = Object.fromEntries(
    data.sheets
      .map((sheet, index) => [index, sheet.rowHeights ?? {}] as const)
      .filter(([, heights]) => Object.keys(heights).length > 0)
  );
}

// ========== Lifecycle ==========
onMounted(async () => {
  const filePath = route.query.file as string;
  if (filePath) {
    console.log('Loading file from path:', filePath);
    await loadFileFromPath(filePath);
  } else {
    await refreshEditorState();
  }
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
      @search-result-click="handleSearchResultClick"
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
              :column-widths="sheetColumnWidths[currentSheetIndex]"
              :row-heights="sheetRowHeights[currentSheetIndex]"
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
      v-if="fileData && !isMobileOrTablet"
      :file-name="fileData.fileName"
      :has-changes="hasUnsavedChanges"
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
