import type { RecentFile } from "@/types";
import { useDocumentSessionStore } from "@/stores/documentSession";
import * as api from "@/api";
import { readFile, openFile, getStorageType } from "@/platform";
import {
  tryAddRecentFileWithResolvedStorage,
  tryRefreshRecentFiles,
  warnRecentFileTrackingFailure,
} from "@/utils/recentFileTracking";
import { fileNameFromPathLike } from "@/utils/fileFormats";

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

    async openFile(path: string): Promise<{ success: boolean; needsRelocate?: boolean }> {
      const existingFile = this.files.find(f => f.path === path);
      const fileName = existingFile?.fileName || fileNameFromPathLike(path, "unknown");

      try {
        const opened = await readFile(path);

        const documentSessionStore = useDocumentSessionStore();
        documentSessionStore.openDocumentResponse(opened, path);

        await tryAddRecentFileWithResolvedStorage(
          {
            path,
            fileName,
            originalPath: existingFile?.originalPath,
          },
          getStorageType
        );
        await tryRefreshRecentFiles(() => this.load());

        return { success: true };
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

        const documentSessionStore = useDocumentSessionStore();
        documentSessionStore.openDocumentResponse(result, result.path);

        await tryAddRecentFileWithResolvedStorage(
          {
            path: result.path,
            fileName: result.fileName,
            originalPath: result.originalPath,
          },
          getStorageType
        );

        try {
          await api.removeRecentFile(file.id);
        } catch (error) {
          warnRecentFileTrackingFailure(error);
        }
        await tryRefreshRecentFiles(() => this.load());
        return true;
      } catch (e) {
        ElMessage.error(`Failed to open file: ${e}`);
        return false;
      }
    },
  },
});
