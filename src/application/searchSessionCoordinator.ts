import type { SearchResponse, SearchSessionSnapshot } from '@/types';

export type SearchSessionPort = {
  beginSearch(query: string): void;
  applySearchResults(response: SearchResponse): void;
  finishSearch(): void;
  clearSearch(): void;
  captureSnapshot(): SearchSessionSnapshot;
  restoreSnapshot(snapshot: SearchSessionSnapshot): void;
};

export function createSearchSessionCoordinator(session: SearchSessionPort) {
  let requestId = 0;

  function beginSearch(query: string): number {
    requestId += 1;
    session.beginSearch(query);
    return requestId;
  }

  function applySearchResults(token: number, response: SearchResponse): boolean {
    if (token !== requestId) return false;
    session.applySearchResults(response);
    return true;
  }

  function finishSearch(token: number) {
    if (token === requestId) session.finishSearch();
  }

  function clearSearch() {
    requestId += 1;
    session.clearSearch();
  }

  function reset() {
    clearSearch();
  }

  function captureSnapshot() {
    return session.captureSnapshot();
  }

  function restoreSnapshot(snapshot: SearchSessionSnapshot) {
    requestId += 1;
    session.restoreSnapshot(snapshot);
  }

  return {
    beginSearch,
    applySearchResults,
    finishSearch,
    clearSearch,
    reset,
    captureSnapshot,
    restoreSnapshot,
  };
}
