import { beforeEach, describe, expect, it } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { useSearchSessionStore } from "@/stores/searchSession";
import type { RuntimeSearchResult, SearchOutcomeStateInput } from '@/types/editorRuntime';
import {
  createDocumentWorkspaceTestContext,
  type DocumentWorkspaceTestContext,
} from '@/test/documentWorkspaceTestContext';

function result(value: string): RuntimeSearchResult {
  return {
    sheetIndex: 0,
    sheetName: "Sheet1",
    row: 0,
    col: 0,
    value,
    cellPosition: "A1",
  };
}

function response(value: string, truncated = false): SearchOutcomeStateInput {
  return { results: [result(value)], truncated };
}

describe("searchSession store", () => {
  let workspace: DocumentWorkspaceTestContext;

  beforeEach(() => {
    setActivePinia(createPinia());
    workspace = createDocumentWorkspaceTestContext();
  });

  it("keeps only the latest search result", () => {
    const store = useSearchSessionStore();
    const coordinator = workspace.runtime.search;
    const first = coordinator.beginSearch("old");
    const second = coordinator.beginSearch("new");

    expect(coordinator.applySearchOutcome(first, response("old"))).toBe(false);
    expect(store.searchResults).toEqual([]);
    expect(store.isSearching).toBe(true);

    expect(coordinator.applySearchOutcome(second, response("new", true))).toBe(true);
    expect(store.searchResults.map((item) => item.value)).toEqual(["new"]);
    expect(store.searchResultsTruncated).toBe(true);
    expect(store.isSearching).toBe(false);
  });

  it("invalidates pending searches when clearing", () => {
    const store = useSearchSessionStore();
    const coordinator = workspace.runtime.search;
    const requestId = coordinator.beginSearch("value");

    coordinator.clearSearch();

    expect(coordinator.applySearchOutcome(requestId, response("value"))).toBe(false);
    expect(store.searchResults).toEqual([]);
    expect(store.searchQuery).toBe("");
    expect(store.isSearching).toBe(false);
  });

  it("clears old results when a new search starts", () => {
    const store = useSearchSessionStore();
    const coordinator = workspace.runtime.search;
    const first = coordinator.beginSearch("old");
    coordinator.applySearchOutcome(first, response("old", true));

    coordinator.beginSearch("new");

    expect(store.searchQuery).toBe("new");
    expect(store.searchResults).toEqual([]);
    expect(store.searchResultsTruncated).toBe(false);
    expect(store.isSearching).toBe(true);
  });

  it("restores results without reviving an invalidated in-flight request", () => {
    const store = useSearchSessionStore();
    const coordinator = workspace.runtime.search;
    const requestId = coordinator.beginSearch("value");
    const snapshot = coordinator.captureSnapshot();

    coordinator.restoreSnapshot(snapshot);

    expect(store.isSearching).toBe(false);
    expect(coordinator.applySearchOutcome(requestId, response("late"))).toBe(false);
  });

  it("keeps request tokens out of serializable UI state", () => {
    const store = useSearchSessionStore();

    workspace.runtime.search.beginSearch("value");

    expect(Object.keys(store.$state)).toEqual([
      "searchResults",
      "searchResultsTruncated",
      "searchQuery",
      "isSearching",
    ]);
  });
});
