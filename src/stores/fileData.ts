import type { FileData } from "@/types";

export const useFileDataStore = defineStore("fileData", {
  state: () => ({
    data: null as FileData | null,
    // 当前打开文件的物理路径（来自 RecentFile.path），用于保存时定位原文件
    currentFilePath: null as string | null,
    documentVersion: 0,
  }),
  actions: {
    set(data: FileData, path: string | null = null) {
      this.data = data;
      this.currentFilePath = path;
      this.documentVersion += 1;
    },
    setData(data: FileData) {
      this.data = data;
    },
    setPath(path: string | null) {
      this.currentFilePath = path;
    },
    clear() {
      this.data = null;
      this.currentFilePath = null;
      this.documentVersion += 1;
    },
  },
});
