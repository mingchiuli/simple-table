import type { RecentFile } from '@/types/recentFileRuntime';

export const useRecentFilesStore = defineStore("recentFiles", {
  state: () => ({
    files: [] as RecentFile[],
    loading: false,
  }),

  actions: {
    replaceFiles(files: RecentFile[]) {
      this.files = files;
    },

    setLoading(loading: boolean) {
      this.loading = loading;
    },
  },
});
