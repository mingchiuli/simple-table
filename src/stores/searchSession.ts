import type { SearchResult } from "@/types";

type SearchSessionRuntime = {
  requestId: number;
};

const searchSessionRuntimes = new WeakMap<object, SearchSessionRuntime>();

export const useSearchSessionStore = defineStore("searchSession", {
  state: () => ({
    searchResults: [] as SearchResult[],
    searchQuery: "",
    isSearching: false,
  }),
  actions: {
    beginSearch(query: string): number {
      const runtime = runtimeFor(this);
      runtime.requestId += 1;
      this.searchQuery = query;
      this.searchResults = [];
      this.isSearching = true;
      return runtime.requestId;
    },
    applySearchResults(requestId: number, results: SearchResult[]): boolean {
      if (requestId !== runtimeFor(this).requestId) {
        return false;
      }
      this.searchResults = results;
      this.isSearching = false;
      return true;
    },
    finishSearch(requestId: number) {
      if (requestId === runtimeFor(this).requestId) {
        this.isSearching = false;
      }
    },
    clearSearch() {
      runtimeFor(this).requestId += 1;
      this.searchResults = [];
      this.searchQuery = "";
      this.isSearching = false;
    },
    reset() {
      this.clearSearch();
    },
  },
});

function runtimeFor(store: object): SearchSessionRuntime {
  let runtime = searchSessionRuntimes.get(store);
  if (!runtime) {
    runtime = { requestId: 0 };
    searchSessionRuntimes.set(store, runtime);
  }
  return runtime;
}
