import { beforeEach, describe, expect, it } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { useSearchSessionStore } from "@/stores/searchSession";
import type { SearchResult } from "@/types";

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

describe("searchSession store", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
  });

  it("keeps only the latest search result", () => {
    const store = useSearchSessionStore();
    const first = store.beginSearch("old");
    const second = store.beginSearch("new");

    expect(store.applySearchResults(first, [result("old")])).toBe(false);
    expect(store.searchResults).toEqual([]);
    expect(store.isSearching).toBe(true);

    expect(store.applySearchResults(second, [result("new")])).toBe(true);
    expect(store.searchResults.map((item) => item.value)).toEqual(["new"]);
    expect(store.isSearching).toBe(false);
  });

  it("invalidates pending searches when clearing", () => {
    const store = useSearchSessionStore();
    const requestId = store.beginSearch("value");

    store.clearSearch();

    expect(store.applySearchResults(requestId, [result("value")])).toBe(false);
    expect(store.searchResults).toEqual([]);
    expect(store.searchQuery).toBe("");
    expect(store.isSearching).toBe(false);
  });

  it("clears old results when a new search starts", () => {
    const store = useSearchSessionStore();
    const first = store.beginSearch("old");
    store.applySearchResults(first, [result("old")]);

    store.beginSearch("new");

    expect(store.searchQuery).toBe("new");
    expect(store.searchResults).toEqual([]);
    expect(store.isSearching).toBe(true);
  });
});
