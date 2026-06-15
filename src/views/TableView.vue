<script setup lang="ts">
import { computed, ref, onMounted } from 'vue';
import { useRoute } from 'vue-router';
import { ElMessage } from 'element-plus';
import { HomeFilled } from '@element-plus/icons-vue';
import { usePlatform } from '@/composables/usePlatform';
import { useFileActions } from '@/composables/useFileActions';
import { cellToEditorString, usePendingCellSave } from '@/composables/usePendingCellSave';
import { useFileDataStore } from '@/stores/fileData';
import { Toolbar, StatusBar } from '@/components/layout';
import TableEditor from '@/components/TableEditor.vue';
import { CellEditor } from '@/components/cell';
import { SearchPanel } from '@/components/search';
import * as api from '@/api';
import { colToLetter } from '@/utils/excel';
import type { CellValue, SortState, SheetData, OperationResult, SearchResult } from '@/types';

const route = useRoute();
const fileDataStore = useFileDataStore();
const { isMobileOrTablet } = usePlatform();

// ========== State refs (must be declared before composables use them) ==========
const isLoading = ref(false);
const isFileLoading = ref(false);
const currentSheetIndex = ref(0);
const hasChanges = ref(false);
const canUndo = ref(false);
const canRedo = ref(false);
const currentSortColumn = ref<SortState | null>(null);
const selectedCell = ref<{ row: number; col: number } | null>(null);
const cellEditorValue = ref<string>('');
const autoScroll = ref(false);
const searchResults = ref<SearchResult[]>([]);
const searchQuery = ref('');
const isSearching = ref(false);
const sheetSelectedCells = ref<Map<number, { row: number; col: number }>>(new Map());

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

// ========== Update editor state ==========
async function updateEditorState() {
  try {
    const state = await api.getEditorState();
    canUndo.value = state?.canUndo ?? false;
    canRedo.value = state?.canRedo ?? false;
  } catch (error) {
    console.error('Failed to get editor state:', error);
  }
}

// ========== Apply operation (for undo/redo) ==========
function applyOperation(result: OperationResult) {
  const data = fileData.value;
  if (!data) return;

  if (currentSortColumn.value) {
    currentSortColumn.value = null;
  }

  switch (result.type) {
    case 'SetCell': {
      const resultData = result.data;
      const sheet = data.sheets[resultData.sheetIndex];
      if (!sheet) break;
      if (sheet.rows[resultData.cell.row]) {
        sheet.rows[resultData.cell.row][resultData.cell.col] = resultData.cell.value;
      }
      break;
    }
    case 'AddSheet': {
      const resultData = result.data;
      const sheetData = resultData.sheetData;
      const sheetIndex = resultData.sheetIndex;
      data.sheets.splice(sheetIndex, 0, sheetData);
      break;
    }
    case 'DeleteSheet': {
      const resultData = result.data;
      data.sheets.splice(resultData.sheetIndex, 1);
      if (currentSheetIndex.value >= data.sheets.length) {
        currentSheetIndex.value = Math.max(0, data.sheets.length - 1);
      }
      break;
    }
    case 'AddRow': {
      const resultData = result.data;
      const sheet = data.sheets[resultData.sheetIndex];
      if (!sheet) break;
      const rowValues = resultData.row?.values || [];
      sheet.rows.splice(resultData.row.index, 0, rowValues);
      break;
    }
    case 'DeleteRow': {
      const resultData = result.data;
      const sheet = data.sheets[resultData.sheetIndex];
      if (!sheet) break;
      sheet.rows.splice(resultData.rowIndex, 1);
      break;
    }
    case 'AddColumn': {
      const resultData = result.data;
      const sheet = data.sheets[resultData.sheetIndex];
      if (!sheet) break;
      const colIndex = resultData.column.index;
      const colData = resultData.colData || [];
      for (let i = 0; i < sheet.rows.length; i++) {
        const value = i < colData.length ? colData[i] : null;
        sheet.rows[i].splice(colIndex, 0, value);
      }
      break;
    }
    case 'DeleteColumn': {
      const resultData = result.data;
      const sheet = data.sheets[resultData.sheetIndex];
      if (!sheet) break;
      for (const row of sheet.rows) {
        row.splice(resultData.columnIndex, 1);
      }
      break;
    }
    case 'SortColumn': {
      const resultData = result.data;
      data.sheets[resultData.sheetIndex] = resultData.sheetData;
      currentSortColumn.value = resultData.sortState;
      break;
    }
  }
}

const {
  flushPendingCellChanges,
  handleCellChange,
  handleCellEditing,
  handleCellEditorSubmit,
  handleDeselectCell,
} = usePendingCellSave({
  fileData,
  currentSheet,
  currentSheetIndex,
  selectedCell,
  cellEditorValue,
  currentSortColumn,
  hasChanges,
  updateEditorState,
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
  hasChanges,
  isLoading,
  isFileLoading,
  flushPendingCellChanges,
  updateEditorState,
});

// ========== Row/Column operations ==========
async function handleAddRow() {
  if (!currentSheet.value) return;
  if (!(await flushPendingCellChanges())) return;

  const colCount = currentSheet.value.rows[0]?.length || 0;
  const newRowIndex = currentSheet.value.rows.length;
  currentSheet.value.rows.push(Array(colCount).fill(null));

  try {
    isLoading.value = true;
    await api.addRow(currentSheetIndex.value, newRowIndex);
    hasChanges.value = true;
    await updateEditorState();
  } catch (error) {
    currentSheet.value.rows.pop();
    ElMessage.error(`Failed to add row: ${error}`);
  } finally {
    isLoading.value = false;
  }
}

async function handleDeleteRow(index: number) {
  if (!currentSheet.value) return;
  if (!(await flushPendingCellChanges())) return;

  const deletedRow = currentSheet.value.rows[index];
  currentSheet.value.rows.splice(index, 1);

  try {
    isLoading.value = true;
    await api.deleteRow(currentSheetIndex.value, index);
    hasChanges.value = true;
    await updateEditorState();
  } catch (error) {
    currentSheet.value.rows.splice(index, 0, deletedRow);
    ElMessage.error(`Failed to delete row: ${error}`);
  } finally {
    isLoading.value = false;
  }
}

async function handleAddColumn() {
  if (!currentSheet.value) return;
  if (!(await flushPendingCellChanges())) return;

  for (const row of currentSheet.value.rows) {
    row.push(null);
  }

  try {
    isLoading.value = true;
    await api.addColumn(currentSheetIndex.value);
    hasChanges.value = true;
    await updateEditorState();
  } catch (error) {
    for (const row of currentSheet.value.rows) {
      row.pop();
    }
    ElMessage.error(`Failed to add column: ${error}`);
  } finally {
    isLoading.value = false;
  }
}

async function handleDeleteColumn(index: number) {
  if (!currentSheet.value) return;
  if (!(await flushPendingCellChanges())) return;

  const deletedCols: CellValue[][] = currentSheet.value.rows.map(row => row.splice(index, 1));

  try {
    isLoading.value = true;
    await api.deleteColumn(currentSheetIndex.value, index);
    hasChanges.value = true;
    await updateEditorState();
  } catch (error) {
    for (let i = 0; i < currentSheet.value.rows.length; i++) {
      currentSheet.value.rows[i].splice(index, 0, ...deletedCols[i]);
    }
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
  const newSheet: SheetData = {
    name: `Sheet${newSheetIndex + 1}`,
    rows: [
      [null, null, null, null, null],
      [null, null, null, null, null],
      [null, null, null, null, null],
      [null, null, null, null, null],
      [null, null, null, null, null],
    ],
    merges: [],
  };
  fileData.value.sheets.push(newSheet);

  try {
    isLoading.value = true;
    await api.addSheet();
    selectedCell.value = null;
    cellEditorValue.value = '';
    currentSheetIndex.value = newSheetIndex;
    hasChanges.value = true;
    await updateEditorState();
  } catch (error) {
    fileData.value.sheets.pop();
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
  const deletedSheet = fileData.value.sheets[deletedIndex];
  fileData.value.sheets.splice(deletedIndex, 1);
  const newIndex = deletedIndex > 0 ? deletedIndex - 1 : 0;
  currentSheetIndex.value = newIndex;

  try {
    isLoading.value = true;
    await api.deleteSheet(deletedIndex);
    hasChanges.value = true;
    await updateEditorState();
  } catch (error) {
    fileData.value.sheets.splice(deletedIndex, 0, deletedSheet);
    currentSheetIndex.value = deletedIndex;
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
    const sheet = fileData.value?.sheets[index];
    cellEditorValue.value = cellToEditorString(sheet?.rows[savedCell.row]?.[savedCell.col]);
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

    const result = await api.undo();
    applyOperation(result);
    hasChanges.value = true;
    await updateEditorState();
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

    const result = await api.redo();
    applyOperation(result);
    hasChanges.value = true;
    await updateEditorState();
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

  const sheet = fileData.value?.sheets[result.sheetIndex];
  cellEditorValue.value = cellToEditorString(sheet?.rows[result.row]?.[result.col]);
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

    const result = await api.sortColumn(
      currentSheetIndex.value,
      colIndex,
      ascending,
      prevSortState
    );
    applyOperation(result);
    hasChanges.value = true;
    await updateEditorState();
  } catch (error) {
    ElMessage.error(`Failed to sort column: ${error}`);
  } finally {
    isLoading.value = false;
  }
}

// ========== Column resize ==========
function handleColumnResize(colIndex: number, width: number) {
  if (!currentSheet.value) return;
  if (!currentSheet.value.columnWidths) {
    currentSheet.value.columnWidths = {};
  }
  currentSheet.value.columnWidths[colIndex] = width;
  hasChanges.value = true;
}

// ========== Lifecycle ==========
onMounted(async () => {
  const filePath = route.query.file as string;
  if (filePath) {
    console.log('Loading file from path:', filePath);
    await loadFileFromPath(filePath);
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
          <CellEditor
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
              :merges="currentSheet?.merges"
              :selected-cell="selectedCell"
              :auto-scroll="autoScroll"
              :sort-state="currentSortColumn"
              :column-widths="currentSheet?.columnWidths"
              @cell-change="handleCellChange"
              @cell-editing="handleCellEditing"
              @delete-row="handleDeleteRow"
              @delete-column="handleDeleteColumn"
              @select-cell="handleSelectCell"
              @sort-column="handleSortColumn"
              @column-resize="handleColumnResize"
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
      :has-changes="hasChanges"
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
