import { defineStore } from "pinia";
import { open } from "@tauri-apps/plugin-dialog";
import { readFile } from "@tauri-apps/plugin-fs";
import { basename } from "@tauri-apps/api/path";
import { ElMessage } from "element-plus";
import type { FileData, RecentFile } from "@/types";
import { useFileDataStore } from "@/stores/fileData";
import * as api from "@/api";

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
      const exists = await api.checkFileExists(path);
      if (!exists) {
        return { success: false, needsRelocate: true };
      }

      try {
        const bytes = await readFile(path);
        const bytesArray = Array.from(bytes);
        const fileData = await api.readFileBytes(path, bytesArray);

        const fileDataStore = useFileDataStore();
        fileDataStore.set(fileData);

        // 获取文件扩展名
        const fileName = await basename(path);
        const extension = fileName.split(".").pop() || "";
        const fileSize = bytes.byteLength;

        // 使用支持 bytes 的 API 生成缩略图（支持移动端）
        await api.addRecentFileWithThumbnail(
          path,
          fileName,
          fileSize,
          bytesArray,
          extension
        );
        await this.load();

        return { success: true, file: fileData };
      } catch (e) {
        ElMessage.error(`Failed to open file: ${e}`);
        return { success: false };
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
        await this.updatePath(file.id, selected);
        const result = await this.openFile(selected);
        return result.success;
      }
      return false;
    },
  },
});
