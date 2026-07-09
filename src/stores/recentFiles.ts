import type { RecentFile } from "@/types";
import * as api from "@/api";

type RecentFilesRuntime = {
  loadRequestId: number;
  activeLoadCount: number;
};

const recentFilesRuntimes = new WeakMap<object, RecentFilesRuntime>();

export const useRecentFilesStore = defineStore("recentFiles", {
  state: () => ({
    files: [] as RecentFile[],
    loading: false,
  }),

  actions: {
    async load() {
      const runtime = recentFilesRuntimeFor(this);
      const requestId = runtime.loadRequestId + 1;
      runtime.loadRequestId = requestId;
      runtime.activeLoadCount += 1;
      this.loading = true;
      try {
        const files = await api.getRecentFiles();
        if (requestId === runtime.loadRequestId) {
          this.files = files;
        }
      } finally {
        runtime.activeLoadCount = Math.max(0, runtime.activeLoadCount - 1);
        this.loading = runtime.activeLoadCount > 0;
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
  },
});

function recentFilesRuntimeFor(store: object): RecentFilesRuntime {
  let runtime = recentFilesRuntimes.get(store);
  if (!runtime) {
    runtime = { loadRequestId: 0, activeLoadCount: 0 };
    recentFilesRuntimes.set(store, runtime);
  }
  return runtime;
}
