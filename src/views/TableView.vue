<script setup lang="ts">
import { ref, computed } from "vue";
import { useRouter } from "vue-router";
import { invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";
import { ElMessage } from "element-plus";
import type { FileData, CellValue } from "@/types";
import { useFileDataStore } from "@/stores/fileData";
import Toolbar from "@/components/Toolbar.vue";
import TableEditor from "@/components/TableEditor.vue";
import StatusBar from "@/components/StatusBar.vue";

const router = useRouter();
const fileDataStore = useFileDataStore();

const currentSheetIndex = ref(0);
const hasChanges = ref(false);
const isLoading = ref(false);

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

function handleCellChange(rowIndex: number, colIndex: number, value: string) {
  if (!fileData.value || !currentSheet.value) return;
  const newValue = parseCellValue(value);
  currentSheet.value.rows[rowIndex][colIndex] = newValue;
  hasChanges.value = true;
}

function handleAddRow() {
  if (!currentSheet.value) return;
  const colCount = columns.value.length;
  currentSheet.value.rows.push(Array(colCount).fill(null));
  hasChanges.value = true;
}

function handleDeleteRow(index: number) {
  if (!currentSheet.value) return;
  currentSheet.value.rows.splice(index, 1);
  hasChanges.value = true;
}

function handleAddColumn() {
  if (!currentSheet.value) return;
  for (const row of currentSheet.value.rows) {
    row.push(null);
  }
  hasChanges.value = true;
}

function handleDeleteColumn() {
  if (!currentSheet.value || !currentSheet.value.rows.length) return;
  for (const row of currentSheet.value.rows) {
    if (row.length > 0) {
      row.pop();
    }
  }
  hasChanges.value = true;
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
      @open-file="handleOpenFile"
      @save-file="handleSaveFile"
      @sheet-change="(i) => (currentSheetIndex = i)"
      @add-row="handleAddRow"
      @add-column="handleAddColumn"
      @delete-column="handleDeleteColumn"
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
