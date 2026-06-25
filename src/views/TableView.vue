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
import type { CellValue, EditorMutationResponse, FileData, SortState, SearchResult } from '@/types';

const route = useRoute();
const fileDataStore = useFileDataStore();
const { isMobileOrTablet } = usePlatform();

// ========== State refs (must be declared before composables use them) ==========
const isLoading = ref(false);
const isFileLoading = ref(false);
const currentSheetIndex = ref(0);
const currentSortColumn = ref<SortState | null>(null);
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

  if (response.operation?.type === 'SortColumn') {
    currentSortColumn.value = response.operation.data.sortState;
  } else if (response.operation) {
    currentSortColumn.value = null;
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
  currentSortColumn,
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
  currentSortColumn.value = null;
  sheetSelectedCells.value = new Map();
  sheetColumnWidths.value = {};
  sheetRowHeights.value = {};
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

// ========== Sort ==========
async function handleSortColumn(colIndex: number, ascending: boolean) {
  if (!fileData.value) return;

  try {
    isLoading.value = true;
    if (!(await flushPendingCellChanges())) return;

    const prevSortState = currentSortColumn.value;
    currentSortColumn.value = null;

    applyMutationResponse(await api.sortColumn(
      currentSheetIndex.value,
      colIndex,
      ascending,
      prevSortState
    ));
  } catch (error) {
    ElMessage.error(`Failed to sort column: ${error}`);
  } finally {
    isLoading.value = false;
  }
}

// ========== Column resize ==========
function handleColumnResize(colIndex: number, width: number) {
  const sheetWidths = sheetColumnWidths.value[currentSheetIndex.value] ?? {};
  sheetColumnWidths.value[currentSheetIndex.value] = {
    ...sheetWidths,
    [colIndex]: width,
  };
}

function handleRowResize(rowIndex: number, height: number) {
  const sheetHeights = sheetRowHeights.value[currentSheetIndex.value] ?? {};
  sheetRowHeights.value[currentSheetIndex.value] = {
    ...sheetHeights,
    [rowIndex]: height,
  };
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
      :search-results="searchResults"
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
              :sort-state="currentSortColumn"
              :column-widths="sheetColumnWidths[currentSheetIndex]"
              :row-heights="sheetRowHeights[currentSheetIndex]"
              @cell-change="handleCellChange"
              @cell-editing="handleCellEditing"
              @cell-edit-cancel="handleCellEditCancel"
              @delete-row="handleDeleteRow"
              @delete-column="handleDeleteColumn"
              @select-cell="handleSelectCell"
              @sort-column="handleSortColumn"
              @column-resize="handleColumnResize"
              @row-resize="handleRowResize"
            />
          </div>
        </template>
      </div>

      <SearchPanel
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
  height: 100dvh;
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
}

.editor-column {
  flex: 1;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  min-width: 0;
}

.table-wrapper {
  background: var(--el-bg-color);
  flex: 1;
  display: flex;
  flex-direction: column;
  overflow-x: auto;
  overflow-y: hidden;
  min-width: 0;
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
</style>
