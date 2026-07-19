import type {
  RuntimeSearchResult,
  SearchOutcomeStateInput,
  SearchSessionSnapshot,
} from '@/types/editorRuntime';

export const useSearchSessionStore = defineStore("searchSession", {
  state: () => ({
    searchResults: [] as RuntimeSearchResult[],
    searchResultsTruncated: false,
    searchQuery: "",
    isSearching: false,
  }),
  actions: {
    beginSearch(query: string) {
      this.searchQuery = query;
      this.searchResults = [];
      this.searchResultsTruncated = false;
      this.isSearching = true;
    },
    applySearchOutcome(outcome: SearchOutcomeStateInput) {
      this.searchResults = outcome.results;
      this.searchResultsTruncated = outcome.truncated;
      this.isSearching = false;
    },
    finishSearch() {
      this.isSearching = false;
    },
    clearSearch() {
      this.searchResults = [];
      this.searchResultsTruncated = false;
      this.searchQuery = "";
      this.isSearching = false;
    },
    reset() {
      this.clearSearch();
    },
    captureSnapshot(): SearchSessionSnapshot {
      return {
        searchResults: [...this.searchResults],
        searchResultsTruncated: this.searchResultsTruncated,
        searchQuery: this.searchQuery,
        isSearching: this.isSearching,
      };
    },
    restoreSnapshot(snapshot: SearchSessionSnapshot) {
      this.searchResults = [...snapshot.searchResults];
      this.searchResultsTruncated = snapshot.searchResultsTruncated;
      this.searchQuery = snapshot.searchQuery;
      this.isSearching = false;
    },
  },
});
