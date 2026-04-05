import { defineStore } from "pinia";
import { open } from "@tauri-apps/plugin-dialog";
import { readFile } from "@tauri-apps/plugin-fs";
import { basename } from "@tauri-apps/api/path";
import { ElMessage } from "element-plus";
import type { FileData, RecentFile } from "@/types";
import { useFileDataStore } from "@/stores/fileData";
import * as api from "@/api";
import { isAndroid, isIOS } from "@/utils/platform";

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
      // Android: 使用持久化 URI 读取
      if (await isAndroid()) {
        try {
          // 获取文件名（从已有记录取，避免从 content:// URI 解析出错）
          const existingFile = this.files.find(f => f.path === path);
          const fileName = existingFile?.fileName || path.split("/").pop()?.split("?")[0] || "unknown";
          const extension = fileName.split(".").pop() || "";

          const bytes = await api.readFileAndroid(path);
          const fileData = await api.readFileBytes(path, bytes, fileName);

          const fileDataStore = useFileDataStore();
          fileDataStore.set(fileData);

          const fileSize = bytes.length;

          await api.addRecentFileWithThumbnail(
            path,
            fileName,
            fileSize,
            bytes,
            extension,
            'androidUri'
          );
          await this.load();

          return { success: true, file: fileData };
        } catch (e) {
          // 权限可能被撤销，需要重新选择文件
          ElMessage.error(`Failed to open file: ${e}`);
          return { success: false, needsRelocate: true };
        }
      }

      // iOS: 从私有目录读取
      if (await isIOS()) {
        const existingFile = this.files.find(f => f.path === path);
        const fileName = existingFile?.fileName || path.split("/").pop() || "unknown";
        const extension = fileName.split(".").pop() || "";

        try {
          const bytes = await readFile(path);
          const bytesArray = Array.from(bytes);
          const fileData = await api.readFileBytes(path, bytesArray, fileName);

          const fileDataStore = useFileDataStore();
          fileDataStore.set(fileData);

          const fileSize = bytes.byteLength;
          const originalPath = existingFile?.originalPath;

          await api.addRecentFileWithThumbnail(
            path,
            fileName,
            fileSize,
            bytesArray,
            extension,
            'iosPrivate',
            originalPath
          );
          await this.load();

          return { success: true, file: fileData };
        } catch (e) {
          ElMessage.error(`Failed to open file: ${e}`);
          return { success: false, needsRelocate: true };
        }
      }

      // 桌面端: 现有逻辑
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
        const fileName = decodeURIComponent(await basename(path));
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
      // Android: 使用专用文件选择器
      if (await isAndroid()) {
        try {
          const result = await api.pickFileAndroid();
          // 更新路径
          await this.updatePath(file.id, result.path);
          // 读取文件（传入正确的文件名）
          const fileData = await api.readFileBytes(result.path, result.bytes, result.fileName);
          const fileDataStore = useFileDataStore();
          fileDataStore.set(fileData);

          const extension = result.fileName.split(".").pop() || "";
          await api.addRecentFileWithThumbnail(
            result.path,
            result.fileName,
            result.bytes.length,
            result.bytes,
            extension,
            'androidUri'
          );
          await this.load();
          return true;
        } catch (e) {
          ElMessage.error(`Failed to open file: ${e}`);
          return false;
        }
      }

      // iOS: 使用专用文件选择器并复制到私有目录
      if (await isIOS()) {
        try {
          const result = await api.pickFileIOS();

          // 从私有路径读取文件
          const bytes = await readFile(result.path);
          const bytesArray = Array.from(bytes);
          const fileData = await api.readFileBytes(result.path, bytesArray, result.fileName);
          const fileDataStore = useFileDataStore();
          fileDataStore.set(fileData);

          // 直接添加/更新最近文件记录（使用新的私有路径）
          const extension = result.fileName.split(".").pop() || "";
          await api.addRecentFileWithThumbnail(
            result.path,
            result.fileName,
            bytesArray.length,
            bytesArray,
            extension,
            'iosPrivate',
            result.originalPath
          );

          // 删除旧记录
          await api.removeRecentFile(file.id);
          await this.load();
          return true;
        } catch (e) {
          ElMessage.error(`Failed to open file: ${e}`);
          return false;
        }
      }

      // 桌面端: 现有逻辑
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
