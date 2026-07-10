import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/api", () => ({
  addRecentFileWithThumbnail: vi.fn(),
}));

describe("recentFileTracking", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("adds recent metadata through the backend recent-file boundary", async () => {
    const api = await import("@/api");
    vi.mocked(api.addRecentFileWithThumbnail).mockResolvedValue({
      id: "recent",
      path: "/tmp/book.xlsx",
      fileName: "book.xlsx",
      lastOpened: 1,
      fileSize: 42,
      storageType: "desktopPath",
    });
    const { tryAddRecentFileWithThumbnail } = await import("@/utils/recentFileTracking");

    await expect(
      tryAddRecentFileWithThumbnail({
        context: { documentId: 7, baseRevision: 3 },
      })
    ).resolves.toBe(true);

    expect(api.addRecentFileWithThumbnail).toHaveBeenCalledWith(
      { documentId: 7, baseRevision: 3 },
      undefined
    );
  });

  it("passes document context for thumbnail generation", async () => {
    const api = await import("@/api");
    vi.mocked(api.addRecentFileWithThumbnail).mockResolvedValue({
      id: "recent",
      path: "/tmp/book.xlsx",
      fileName: "book.xlsx",
      lastOpened: 1,
      fileSize: 42,
      storageType: "desktopPath",
    });
    const { tryAddRecentFileWithThumbnail } = await import("@/utils/recentFileTracking");

    await expect(
      tryAddRecentFileWithThumbnail({
        originalPath: "/original/book.xlsx",
        context: { documentId: 8, baseRevision: 4 },
      })
    ).resolves.toBe(true);

    expect(api.addRecentFileWithThumbnail).toHaveBeenCalledWith(
      { documentId: 8, baseRevision: 4 },
      "/original/book.xlsx"
    );
  });

  it("returns false instead of throwing when metadata update fails", async () => {
    const api = await import("@/api");
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    vi.mocked(api.addRecentFileWithThumbnail).mockRejectedValue(
      new Error("metadata unavailable")
    );
    const { tryAddRecentFileWithThumbnail } = await import("@/utils/recentFileTracking");

    await expect(
      tryAddRecentFileWithThumbnail({
        context: { documentId: 7, baseRevision: 3 },
      })
    ).resolves.toBe(false);

    expect(warn).toHaveBeenCalled();
    expect(api.addRecentFileWithThumbnail).toHaveBeenCalledTimes(1);
  });

  it("keeps refresh failures out of the caller's main flow", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const { tryRefreshRecentFiles } = await import("@/utils/recentFileTracking");

    await expect(
      tryRefreshRecentFiles(async () => {
        throw new Error("store unavailable");
      })
    ).resolves.toBe(false);

    expect(warn).toHaveBeenCalled();
  });

});
