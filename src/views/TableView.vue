<script setup lang="ts">
import { ref, computed, watch } from "vue";
import { useRouter } from "vue-router";
import { invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";
import { ElMessage } from "element-plus";
import { HomeFilled } from "@element-plus/icons-vue";
import type { FileData, CellValue, OperationResult, SearchResult } from "@/types";
import { useFileDataStore } from "@/stores/fileData";
import Toolbar from "@/components/Toolbar.vue";
import TableEditor from "@/components/TableEditor.vue";
import StatusBar from "@/components/StatusBar.vue";
import CellEditor from "@/components/CellEditor.vue";
import SearchPanel from "@/components/SearchPanel.vue";

const router = useRouter();
const fileDataStore = useFileDataStore();

const currentSheetIndex = ref(0);
const hasChanges = ref(false);
const isLoading = ref(false);
const canUndo = ref(false);
const canRedo = ref(false);
const searchResults = ref<SearchResult[]>([]);
const isSearching = ref(false);
const selectedCell = ref<{ row: number; col: number } | null>(null);
const cellEditorValue = ref<string>("");
const autoScroll = ref(false);

// Store selected cell for each sheet
const sheetSelectedCells = ref<Map<number, { row: number; col: number }>>(new Map());

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
  return Array.from({ length: maxCols }, (_, i) => {
    const charCode = 65 + i;
    return String.fromCharCode(charCode);
  });
});

const sheetNames = computed(() => {
  if (!fileData.value) return [];
  return fileData.value.sheets.map((s) => s.name);
});

function parseCellValue(value: string): CellValue {
  if (value === "") return null;
  const num = Number(value);
  if (!isNaN(num)) return num;
  if (value.toLowerCase() === "true") return true;
  if (value.toLowerCase() === "false") return false;
  return value;
}

function toRustCellValue(value: CellValue): string | number | boolean | null {
  return value;
}

// 获取当前选中单元格的值
const currentCellValue = computed(() => {
  if (!selectedCell.value || !currentSheet.value) return null;
  const { row, col } = selectedCell.value;
  return currentSheet.value.rows[row]?.[col] ?? null;
});

// 监听选中单元格变化，更新编辑输入框
watch(
  () => selectedCell.value,
  (newCell) => {
    if (newCell) {
      const value = currentCellValue.value;
      cellEditorValue.value = value !== null ? String(value) : "";
    } else {
      cellEditorValue.value = "";
    }
  },
  { immediate: true }
);

// 监听编辑输入框变化，实时更新单元格
let debounceTimer: ReturnType<typeof setTimeout> | null = null;
watch(cellEditorValue, (newValue) => {
  if (!selectedCell.value || !currentSheet.value) return;

  const { row, col } = selectedCell.value;
  const originalValue = currentSheet.value.rows[row]?.[col];

  // 清除之前的定时器
  if (debounceTimer) {
    clearTimeout(debounceTimer);
  }

  // 防抖处理，避免每次输入都调用 API
  debounceTimer = setTimeout(() => {
    const newValueStr = newValue;
    const originalValueStr = originalValue !== null ? String(originalValue) : "";

    if (newValueStr !== originalValueStr) {
      handleCellChange(row, col, newValueStr);
    }
  }, 300);
});

// 根据增量结果更新本地数据（直接修改，避免深拷贝）
// Rust 使用 #[serde(tag = "type", content = "data")]，所以需要从 result.data 获取内容
function applyOperation(result: OperationResult) {
  const data = fileData.value;
  if (!data) return;

  const resultData = (result as any).data;
  if (!resultData) return;

  const sheet = data.sheets[resultData.sheet_index];
  if (!sheet) return;

  switch (result.type) {
    case "SetCell": {
      const { row, col, value } = resultData.cell;
      if (sheet.rows[row]) {
        sheet.rows[row][col] = value;
      }
      break;
    }
    case "AddRow": {
      const colCount = sheet.rows[0]?.length || 0;
      sheet.rows.splice(resultData.row.index, 0, Array(colCount).fill(null));
      break;
    }
    case "DeleteRow": {
      sheet.rows.splice(resultData.row_index, 1);
      break;
    }
    case "AddColumn": {
      const colIndex = resultData.column.index;
      for (const row of sheet.rows) {
        row.splice(colIndex + 1, 0, null);
      }
      break;
    }
    case "DeleteColumn": {
      for (const row of sheet.rows) {
        row.splice(resultData.column_index, 1);
      }
      break;
    }
    case "AddSheet": {
      // Sheet is already added via get_file_data, no need to modify
      break;
    }
    case "DeleteSheet": {
      // Sheet is already deleted via get_file_data, no need to modify
      break;
    }
    case "Batch": {
      for (const change of resultData.changes) {
        if (sheet.rows[change.row]) {
          sheet.rows[change.row][change.col] = change.value;
        }
      }
      break;
    }
  }

  // 直接修改 in-place，Vue 响应式会自动检测到变化
}

async function updateEditorState() {
  try {
    const state = await invoke<{ can_undo: boolean; can_redo: boolean }>("get_editor_state");
    canUndo.value = state.can_undo;
    canRedo.value = state.can_redo;
  } catch (error) {
    console.error("Failed to get editor state:", error);
  }
}

async function handleOpenFile() {
  try {
    const selected = await open({
      multiple: false,
      filters: [
        {
          name: "Spreadsheet",
          extensions: ["xlsx", "xls", "csv", "ods"],
        },
      ],
    });

    if (selected) {
      isLoading.value = true;
      const result = await invoke<FileData>("read_file", { path: selected });
      fileDataStore.set(result);
      currentSheetIndex.value = 0;
      hasChanges.value = false;
      await updateEditorState();
      ElMessage.success("File loaded successfully");
    }
  } catch (error) {
    ElMessage.error(`Failed to open file: ${error}`);
  } finally {
    isLoading.value = false;
  }
}

async function handleSaveFile() {
  if (!fileData.value) return;

  try {
    const originalExtension = fileData.value.file_name.split(".").pop() || "xlsx";
    const isNewFile = fileData.value.file_name.startsWith("untitled");
    const defaultName = isNewFile
      ? "untitled"
      : fileData.value.file_name.replace(/\.[^.]+$/, "");

    // Determine available extensions based on file type
    let extensions: string[];
    if (isNewFile) {
      // New file: allow选择 xlsx or csv
      extensions = ["xlsx", "csv"];
    } else if (originalExtension === "csv") {
      extensions = ["csv"];
    } else {
      extensions = ["xlsx"];
    }

    const savePath = await save({
      defaultPath: `${defaultName}.${extensions[0]}`,
      filters: [
        {
          name: "Spreadsheet",
          extensions,
        },
      ],
    });

    if (savePath) {
      isLoading.value = true;
      await invoke("save_file", { path: savePath, fileData: fileData.value });
      hasChanges.value = false;
      ElMessage.success("File saved successfully");
    }
  } catch (error) {
    ElMessage.error(`Failed to save file: ${error}`);
  } finally {
    isLoading.value = false;
  }
}

async function handleCellChange(rowIndex: number, colIndex: number, value: string) {
  if (!fileData.value || !currentSheet.value) return;

  const oldValue = currentSheet.value.rows[rowIndex][colIndex];
  const newValue = parseCellValue(value);

  // 检查是否是当前选中的单元格，如果是则同步更新上方编辑栏
  const isCurrentCell = selectedCell.value?.row === rowIndex && selectedCell.value?.col === colIndex;

  try {
    isLoading.value = true;
    const result = await invoke<OperationResult>("set_cell", {
      sheetIndex: currentSheetIndex.value,
      row: rowIndex,
      col: colIndex,
      oldValue: toRustCellValue(oldValue),
      newValue: toRustCellValue(newValue),
    });
    applyOperation(result);

    // 同步更新上方编辑栏的值
    if (isCurrentCell) {
      cellEditorValue.value = value;
    }

    hasChanges.value = true;
    await updateEditorState();
  } catch (error) {
    ElMessage.error(`Failed to set cell: ${error}`);
  } finally {
    isLoading.value = false;
  }
}

async function handleAddRow() {
  if (!currentSheet.value) return;

  try {
    isLoading.value = true;
    const result = await invoke<OperationResult>("add_row", {
      sheetIndex: currentSheetIndex.value,
      rowIndex: currentSheet.value.rows.length,
    });
    applyOperation(result);
    hasChanges.value = true;
    await updateEditorState();
  } catch (error) {
    ElMessage.error(`Failed to add row: ${error}`);
  } finally {
    isLoading.value = false;
  }
}

async function handleDeleteRow(index: number) {
  if (!currentSheet.value) return;

  const rowData = currentSheet.value.rows[index];

  try {
    isLoading.value = true;
    const result = await invoke<OperationResult>("delete_row", {
      sheetIndex: currentSheetIndex.value,
      rowIndex: index,
      rowData: rowData.map(toRustCellValue),
    });
    applyOperation(result);
    hasChanges.value = true;
    await updateEditorState();
  } catch (error) {
    ElMessage.error(`Failed to delete row: ${error}`);
  } finally {
    isLoading.value = false;
  }
}

async function handleAddColumn() {
  if (!currentSheet.value) return;

  try {
    isLoading.value = true;
    const result = await invoke<OperationResult>("add_column", {
      sheetIndex: currentSheetIndex.value,
    });
    applyOperation(result);
    hasChanges.value = true;
    await updateEditorState();
  } catch (error) {
    ElMessage.error(`Failed to add column: ${error}`);
  } finally {
    isLoading.value = false;
  }
}

async function handleDeleteColumn(index: number) {
  if (!currentSheet.value) return;

  const colData = currentSheet.value.rows.map(row => row[index]);

  try {
    isLoading.value = true;
    const result = await invoke<OperationResult>("delete_column", {
      sheetIndex: currentSheetIndex.value,
      colIndex: index,
      colData: colData.map(toRustCellValue),
    });
    applyOperation(result);
    hasChanges.value = true;
    await updateEditorState();
  } catch (error) {
    ElMessage.error(`Failed to delete column: ${error}`);
  } finally {
    isLoading.value = false;
  }
}

async function handleAddSheet() {
  if (!fileData.value) return;

  try {
    isLoading.value = true;
    const result = await invoke<OperationResult>("add_sheet");
    // Get the updated file data from backend to avoid duplication
    const updatedData = await invoke<FileData>("get_file_data");
    fileDataStore.set(updatedData);
    // Switch to the new sheet
    const resultData = (result as any).data;
    if (resultData && resultData.sheet_index !== undefined) {
      // Clear selected cell and editor when switching to new sheet
      selectedCell.value = null;
      cellEditorValue.value = "";
      currentSheetIndex.value = resultData.sheet_index;
    }
    hasChanges.value = true;
    await updateEditorState();
  } catch (error) {
    ElMessage.error(`Failed to add sheet: ${error}`);
  } finally {
    isLoading.value = false;
  }
}

async function handleDeleteSheet() {
  if (!fileData.value || fileData.value.sheets.length <= 1) {
    ElMessage.warning("Cannot delete the last sheet");
    return;
  }

  try {
    isLoading.value = true;
    const result = await invoke<OperationResult>("delete_sheet", {
      sheetIndex: currentSheetIndex.value,
    });
    // Get the updated file data from backend to avoid mismatch
    const updatedData = await invoke<FileData>("get_file_data");
    fileDataStore.set(updatedData);
    // Switch to the new current sheet
    const resultData = (result as any).data;
    if (resultData && resultData.sheet_index !== undefined) {
      currentSheetIndex.value = resultData.sheet_index;
    }
    hasChanges.value = true;
    await updateEditorState();
  } catch (error) {
    ElMessage.error(`Failed to delete sheet: ${error}`);
  } finally {
    isLoading.value = false;
  }
}

async function handleUndo() {
  if (!canUndo.value) return;

  try {
    isLoading.value = true;
    const result = await invoke<OperationResult>("undo");
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
    const result = await invoke<OperationResult>("redo");
    applyOperation(result);
    hasChanges.value = true;
    await updateEditorState();
  } catch (error) {
    ElMessage.error(`Failed to redo: ${error}`);
  } finally {
    isLoading.value = false;
  }
}

function handleBack() {
  fileDataStore.clear();
  router.push({ name: "home" });
}

async function handleSearch(query: string, scope: "currentSheet" | "allSheets") {
  if (!fileData.value) return;

  try {
    isSearching.value = true;
    const results = await invoke<SearchResult[]>("search", {
      query,
      scope,
      currentSheetIndex: scope === "currentSheet" ? currentSheetIndex.value : null,
    });
    searchResults.value = results;
  } catch (error) {
    ElMessage.error(`Search failed: ${error}`);
  } finally {
    isSearching.value = false;
  }
}

function handleSearchResultClick(result: SearchResult) {
  // 切换到对应的 sheet
  if (result.sheet_index !== currentSheetIndex.value) {
    currentSheetIndex.value = result.sheet_index;
  }
  // 选中对应的单元格，并触发滚动到中央
  autoScroll.value = true;
  selectedCell.value = { row: result.row, col: result.col };
}

function handleClearSearch() {
  searchResults.value = [];
}

function handleSheetChange(index: number) {
  // Save current selected cell for the current sheet
  if (selectedCell.value !== null) {
    sheetSelectedCells.value.set(currentSheetIndex.value, selectedCell.value);
  }

  // Clear cell editor when switching sheets
  cellEditorValue.value = "";
  currentSheetIndex.value = index;

  // Restore selected cell for the new sheet if it was previously saved
  const savedCell = sheetSelectedCells.value.get(index);
  if (savedCell) {
    selectedCell.value = savedCell;
    // Update cell editor value with the new sheet's cell value
    const sheet = fileData.value?.sheets[index];
    if (sheet && sheet.rows[savedCell.row] && sheet.rows[savedCell.row][savedCell.col] !== null) {
      cellEditorValue.value = String(sheet.rows[savedCell.row][savedCell.col]);
    }
    // Trigger auto scroll to the selected cell
    autoScroll.value = true;
  } else {
    selectedCell.value = null;
  }
}

// 按下回车或失焦时提交编辑
function handleCellEditorSubmit() {
  if (!selectedCell.value) return;
  const { row, col } = selectedCell.value;
  handleCellChange(row, col, cellEditorValue.value);
}
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
      <div class="table-wrapper">
        <CellEditor
          v-if="selectedCell && fileData"
          v-model="cellEditorValue"
          :cell-position="selectedCell"
          @submit="handleCellEditorSubmit"
        />

        <TableEditor
          :data="tableData"
          :columns="columns"
          :selected-cell="selectedCell"
          :auto-scroll="autoScroll"
          @cell-change="handleCellChange"
          @cell-editing="(row, col, value) => { if (selectedCell?.row === row && selectedCell?.col === col) cellEditorValue = value }"
          @delete-row="handleDeleteRow"
          @delete-column="handleDeleteColumn"
          @select-cell="(row, col) => { autoScroll = false; selectedCell = { row, col } }"
        />
      </div>

      <!-- Search Results Panel -->
      <SearchPanel
        :results="searchResults"
        @result-click="handleSearchResultClick"
        @clear="handleClearSearch"
      />
    </main>

    <StatusBar
      v-if="fileData"
      :file-name="fileData.file_name"
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
  height: 100vh;
  background-color: #fff;
  position: relative;
}

.content {
  flex: 1;
  overflow: auto;
  padding: 0;
  display: flex;
  flex-direction: row;
}

.table-wrapper {
  background: #fff;
  flex: 1;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.back-btn {
  position: absolute;
  bottom: 20px;
  left: 20px;
  z-index: 100;
}
</style>
