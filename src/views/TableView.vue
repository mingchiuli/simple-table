<script setup lang="ts">
import { ref, computed } from "vue";
import { useRouter } from "vue-router";
import { invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";
import { ElMessage } from "element-plus";
import type { FileData, CellValue, OperationResult } from "@/types";
import { useFileDataStore } from "@/stores/fileData";
import Toolbar from "@/components/Toolbar.vue";
import TableEditor from "@/components/TableEditor.vue";
import StatusBar from "@/components/StatusBar.vue";

const router = useRouter();
const fileDataStore = useFileDataStore();

const currentSheetIndex = ref(0);
const hasChanges = ref(false);
const isLoading = ref(false);
const canUndo = ref(false);
const canRedo = ref(false);

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
    const extension = fileData.value.file_name.split(".").pop() || "xlsx";
    const defaultName = fileData.value.file_name.replace(/\.[^.]+$/, "_edited");

    const savePath = await save({
      defaultPath: `${defaultName}.${extension}`,
      filters: [
        {
          name: "Spreadsheet",
          extensions: [extension === "csv" ? "csv" : "xlsx"],
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

async function handleDeleteColumn() {
  if (!currentSheet.value || !currentSheet.value.rows.length) return;

  try {
    isLoading.value = true;
    const result = await invoke<OperationResult>("delete_column", {
      sheetIndex: currentSheetIndex.value,
      colIndex: currentSheet.value.rows[0].length - 1,
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
</script>

<template>
  <div class="app-container">
    <Toolbar
      :file-data="fileData"
      :sheet-names="sheetNames"
      :current-sheet-index="currentSheetIndex"
      :column-count="columns.length"
      :can-undo="canUndo"
      :can-redo="canRedo"
      @open-file="handleOpenFile"
      @save-file="handleSaveFile"
      @sheet-change="(i) => (currentSheetIndex = i)"
      @add-row="handleAddRow"
      @add-column="handleAddColumn"
      @delete-column="handleDeleteColumn"
      @undo="handleUndo"
      @redo="handleRedo"
    />

    <main class="content">
      <div class="table-wrapper">
        <TableEditor
          :data="tableData"
          :columns="columns"
          @cell-change="handleCellChange"
          @delete-row="handleDeleteRow"
        />
      </div>
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
  flex-direction: column;
}

.table-wrapper {
  background: #fff;
  flex: 1;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.el-table {
  font-size: 14px;
}

.back-btn {
  position: absolute;
  bottom: 20px;
  left: 20px;
  z-index: 100;
}
</style>
