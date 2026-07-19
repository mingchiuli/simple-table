import type {
  SearchOutcomeStateInput,
  SearchSessionSnapshot,
} from '@/types/editorRuntime';

export type SearchSessionPort = {
  beginSearch(query: string): void;
  applySearchOutcome(outcome: SearchOutcomeStateInput): void;
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

  function applySearchOutcome(token: number, outcome: SearchOutcomeStateInput): boolean {
    if (token !== requestId) return false;
    session.applySearchOutcome(outcome);
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
    applySearchOutcome,
    finishSearch,
    clearSearch,
    reset,
    captureSnapshot,
    restoreSnapshot,
  };
}
