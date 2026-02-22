<script setup lang="ts">
import { useRouter } from "vue-router";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { ElMessage } from "element-plus";
import type { FileData } from "@/types";
import { useFileDataStore } from "@/stores/fileData";

const router = useRouter();
const fileDataStore = useFileDataStore();

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
      const result = await invoke<FileData>("read_file", { path: selected });
      fileDataStore.set(result);
      router.push({ name: "table" });
      ElMessage.success("File loaded successfully");
    }
  } catch (error) {
    ElMessage.error(`Failed to open file: ${error}`);
  }
}
</script>

<template>
  <div class="home-view">
    <div class="empty-state">
      <el-icon class="empty-icon"><Document /></el-icon>
      <p>No file opened</p>
      <el-button type="primary" @click="handleOpenFile">
        Open Excel or CSV file
      </el-button>
    </div>
  </div>
</template>

<style scoped>
.home-view {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  height: 100vh;
  background-color: #fff;
}

.empty-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  color: #909399;
}

.empty-icon {
  font-size: 64px;
  margin-bottom: 16px;
}

.empty-state p {
  font-size: 16px;
  margin-bottom: 20px;
}
</style>
