import { beforeEach, describe, expect, it } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { useSearchSessionStore } from "@/stores/searchSession";
import { useSearchSessionCoordinator } from '@/composables/useSearchSessionCoordinator';
import type { SearchResponse, SearchResult } from "@/types";

function result(value: string): SearchResult {
  return {
    sheetIndex: 0,
    sheetName: "Sheet1",
    row: 0,
    col: 0,
    value,
    cellPosition: "A1",
  };
}

function response(value: string, truncated = false): SearchResponse {
  return { results: [result(value)], truncated };
}

describe("searchSession store", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
  });

  it("keeps only the latest search result", () => {
    const store = useSearchSessionStore();
    const coordinator = useSearchSessionCoordinator();
    const first = coordinator.beginSearch("old");
    const second = coordinator.beginSearch("new");

    expect(coordinator.applySearchResults(first, response("old"))).toBe(false);
    expect(store.searchResults).toEqual([]);
    expect(store.isSearching).toBe(true);

    expect(coordinator.applySearchResults(second, response("new", true))).toBe(true);
    expect(store.searchResults.map((item) => item.value)).toEqual(["new"]);
    expect(store.searchResultsTruncated).toBe(true);
    expect(store.isSearching).toBe(false);
  });

  it("invalidates pending searches when clearing", () => {
    const store = useSearchSessionStore();
    const coordinator = useSearchSessionCoordinator();
    const requestId = coordinator.beginSearch("value");

    coordinator.clearSearch();

    expect(coordinator.applySearchResults(requestId, response("value"))).toBe(false);
    expect(store.searchResults).toEqual([]);
    expect(store.searchQuery).toBe("");
    expect(store.isSearching).toBe(false);
  });

  it("clears old results when a new search starts", () => {
    const store = useSearchSessionStore();
    const coordinator = useSearchSessionCoordinator();
    const first = coordinator.beginSearch("old");
    coordinator.applySearchResults(first, response("old", true));

    coordinator.beginSearch("new");

    expect(store.searchQuery).toBe("new");
    expect(store.searchResults).toEqual([]);
    expect(store.searchResultsTruncated).toBe(false);
    expect(store.isSearching).toBe(true);
  });

  it("restores results without reviving an invalidated in-flight request", () => {
    const store = useSearchSessionStore();
    const coordinator = useSearchSessionCoordinator();
    const requestId = coordinator.beginSearch("value");
    const snapshot = coordinator.captureSnapshot();

    coordinator.restoreSnapshot(snapshot);

    expect(store.isSearching).toBe(false);
    expect(coordinator.applySearchResults(requestId, response("late"))).toBe(false);
  });

  it("keeps request tokens out of serializable UI state", () => {
    const store = useSearchSessionStore();

    useSearchSessionCoordinator().beginSearch("value");

    expect(Object.keys(store.$state)).toEqual([
      "searchResults",
      "searchResultsTruncated",
      "searchQuery",
      "isSearching",
    ]);
  });
});
