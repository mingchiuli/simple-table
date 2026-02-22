import { defineStore } from "pinia";
import type { FileData } from "@/types";

export const useFileDataStore = defineStore("fileData", {
  state: () => ({
    data: null as FileData | null,
  }),
  actions: {
    set(data: FileData) {
      this.data = data;
    },
    clear() {
      this.data = null;
    },
  },
});
