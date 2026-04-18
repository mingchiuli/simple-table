import { defineStore } from "pinia";
import { ElMessage } from "element-plus";
import type { FileData, RecentFile } from "@/types";
import { useFileDataStore } from "@/stores/fileData";
import * as api from "@/api";
import { readFile, getPlatformAPI, getStorageType } from "@/platform";

export const useRecentFilesStore = defineStore("recentFiles", {
  state: () => ({
    files: [] as RecentFile[],
    loading: false,
  }),

  actions: {
    async load() {
      this.loading = true;
      try {
        this.files = await api.getRecentFiles();
      } finally {
        this.loading = false;
      }
    },

    async openFile(path: string): Promise<{ success: boolean; file?: FileData; needsRelocate?: boolean }> {
      const existingFile = this.files.find(f => f.path === path);
      const fileName = existingFile?.fileName || path.split("/").pop()?.split("?")[0] || "unknown";
      const extension = fileName.split(".").pop() || "";

      try {
        const bytes = await readFile(path);
        const bytesArray = Array.from(bytes);
        const fileData = await api.readFileBytes(path, bytesArray, fileName);

        const fileDataStore = useFileDataStore();
        fileDataStore.set(fileData);

        const fileSize = bytes.length;
        const storageType = await getStorageType();

        await api.addRecentFileWithThumbnail(
          path,
          fileName,
          fileSize,
          bytesArray,
          extension,
          storageType
        );
        await this.load();

        return { success: true, file: fileData };
      } catch (e) {
        ElMessage.error(`Failed to open file: ${e}`);
        return { success: false, needsRelocate: true };
      }
    },

    async remove(id: string) {
      await api.removeRecentFile(id);
      await this.load();
    },

    async updatePath(id: string, newPath: string) {
      await api.updateRecentFilePath(id, newPath);
      await this.load();
    },

    async relocateAndOpen(file: RecentFile): Promise<boolean> {
      try {
        const api2 = await getPlatformAPI();
        const result = await api2.fileOps.pickFile();
        if (!result) {
          // 用户取消选择
          return false;
        }

        // 读取文件
        const bytes = await api2.fileOps.readFile(result.path);
        const bytesArray = Array.from(bytes);
        const fileData = await api.readFileBytes(result.path, bytesArray, result.fileName);
        const fileDataStore = useFileDataStore();
        fileDataStore.set(fileData);

        const extension = result.fileName.split(".").pop() || "";
        const storageType = await getStorageType();

        await api.addRecentFileWithThumbnail(
          result.path,
          result.fileName,
          bytesArray.length,
          bytesArray,
          extension,
          storageType
        );

        await api.removeRecentFile(file.id);
        await this.load();
        return true;
      } catch (e) {
        ElMessage.error(`Failed to open file: ${e}`);
        return false;
      }
    },
  },
});
