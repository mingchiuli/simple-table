import { beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { useRecentFilesStore } from "@/stores/recentFiles";
import type { RecentFile } from "@/types";

vi.mock("@/api", () => ({
  getRecentFiles: vi.fn(),
  removeRecentFile: vi.fn(),
}));

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((promiseResolve, promiseReject) => {
    resolve = promiseResolve;
    reject = promiseReject;
  });
  return { promise, resolve, reject };
}

function recentFile(id: string): RecentFile {
  return {
    id,
    path: `/tmp/${id}.xlsx`,
    fileName: `${id}.xlsx`,
    lastOpened: 1,
    fileSize: 1,
    storageType: "desktopPath",
  };
}

describe("recentFiles store", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
  });

  it("keeps the newest load result when concurrent loads resolve out of order", async () => {
    const api = await import("@/api");
    const firstLoad = deferred<RecentFile[]>();
    const secondLoad = deferred<RecentFile[]>();
    vi.mocked(api.getRecentFiles)
      .mockReturnValueOnce(firstLoad.promise)
      .mockReturnValueOnce(secondLoad.promise);
    const store = useRecentFilesStore();

    const first = store.load();
    const second = store.load();

    expect(store.loading).toBe(true);

    secondLoad.resolve([recentFile("new")]);
    await second;

    expect(store.files.map((file) => file.id)).toEqual(["new"]);
    expect(store.loading).toBe(true);

    firstLoad.resolve([recentFile("old")]);
    await first;

    expect(store.files.map((file) => file.id)).toEqual(["new"]);
    expect(store.loading).toBe(false);
  });

  it("keeps loading true until all concurrent loads settle", async () => {
    const api = await import("@/api");
    const firstLoad = deferred<RecentFile[]>();
    const secondLoad = deferred<RecentFile[]>();
    vi.mocked(api.getRecentFiles)
      .mockReturnValueOnce(firstLoad.promise)
      .mockReturnValueOnce(secondLoad.promise);
    const store = useRecentFilesStore();

    const first = store.load();
    const second = store.load();

    firstLoad.resolve([recentFile("old")]);
    await first;

    expect(store.loading).toBe(true);
    expect(store.files).toEqual([]);

    secondLoad.resolve([recentFile("new")]);
    await second;

    expect(store.loading).toBe(false);
    expect(store.files.map((file) => file.id)).toEqual(["new"]);
  });
});
