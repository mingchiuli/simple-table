import type { SearchResult } from './generated';

export type DocumentSessionLifecycle = 'idle' | 'loading' | 'saving' | 'closing';

export type SearchSessionSnapshot = {
  searchResults: SearchResult[];
  searchResultsTruncated: boolean;
  searchQuery: string;
  isSearching: boolean;
};
