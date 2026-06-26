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

      try {
        // 直接调用 readFile（现在返回 FileData）
        const fileData = await readFile(path);

        const fileDataStore = useFileDataStore();
        fileDataStore.set(fileData, path);

        const storageType = await getStorageType();

        const bytes = await api.generateCurrentThumbnailBytes();

        const fileSize = await api.getFileSize(path);
        await api.addRecentFileWithThumbnail(
          path,
          fileName,
          fileSize,
          bytes,
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

        const storageType = await getStorageType();

        const bytes = await api.generateCurrentThumbnailBytes();
        const fileSize = await api.getFileSize(result.path);
        await api.addRecentFileWithThumbnail(
          result.path,
          result.fileName,
          fileSize,
          bytes,
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
