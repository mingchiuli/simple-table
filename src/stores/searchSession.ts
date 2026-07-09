import type { SearchResult } from "@/types";

export const useSearchSessionStore = defineStore("searchSession", {
  state: () => ({
    searchResults: [] as SearchResult[],
    searchQuery: "",
    isSearching: false,
    requestId: 0,
  }),
  actions: {
    beginSearch(query: string): number {
      this.requestId += 1;
      this.searchQuery = query;
      this.isSearching = true;
      return this.requestId;
    },
    applySearchResults(requestId: number, results: SearchResult[]): boolean {
      if (requestId !== this.requestId) {
        return false;
      }
      this.searchResults = results;
      this.isSearching = false;
      return true;
    },
    finishSearch(requestId: number) {
      if (requestId === this.requestId) {
        this.isSearching = false;
      }
    },
    clearSearch() {
      this.requestId += 1;
      this.searchResults = [];
      this.searchQuery = "";
      this.isSearching = false;
    },
    reset() {
      this.clearSearch();
    },
  },
});
