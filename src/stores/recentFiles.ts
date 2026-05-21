import { defineStore } from "pinia";
import { ElMessage } from "element-plus";
import type { FileData, RecentFile } from "@/types";
import { useFileDataStore } from "@/stores/fileData";
import * as api from "@/api";
import { readFile, openFile, getStorageType } from "@/platform";

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
        // 直接调用 readFile（现在返回 FileData）
        const fileData = await readFile(path);

        const fileDataStore = useFileDataStore();
        fileDataStore.set(fileData, path);

        const storageType = await getStorageType();

        // Only desktop reads thumbnail bytes in the frontend; mobile import returns
        // bytes from the backend when the file is opened or relocated.
        let bytes: number[] = [];
        if (storageType === 'desktopPath') {
          const { readFile: fsReadFile } = await import('@tauri-apps/plugin-fs');
          bytes = Array.from(await fsReadFile(path));
        }

        await api.addRecentFileWithThumbnail(
          path,
          fileName,
          bytes.length,
          bytes,
          extension,
          storageType,
          existingFile?.originalPath
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
        const result = await openFile();
        if (!result) {
          // 用户取消选择
          return false;
        }

        const fileDataStore = useFileDataStore();
        fileDataStore.set(result.fileData, result.path);

        const extension = result.fileName.split(".").pop() || "";
        const storageType = await getStorageType();

        await api.addRecentFileWithThumbnail(
          result.path,
          result.fileName,
          result.bytes?.length || 0,
          result.bytes || [],
          extension,
          storageType,
          result.originalPath
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
