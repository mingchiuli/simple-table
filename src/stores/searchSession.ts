import type { SearchResult } from "@/types";

export const useSearchSessionStore = defineStore("searchSession", {
  state: () => ({
    searchResults: [] as SearchResult[],
    searchQuery: "",
    isSearching: false,
  }),
  actions: {
    clearSearch() {
      this.searchResults = [];
      this.searchQuery = "";
    },
    reset() {
      this.clearSearch();
      this.isSearching = false;
    },
  },
});
