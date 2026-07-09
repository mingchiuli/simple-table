import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/api", () => ({
  getFileSize: vi.fn(),
  addRecentFileWithThumbnail: vi.fn(),
}));

describe("recentFileTracking", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("adds recent metadata with the current file size", async () => {
    const api = await import("@/api");
    vi.mocked(api.getFileSize).mockResolvedValue(42);
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
        path: "/tmp/book.xlsx",
        fileName: "book.xlsx",
        storageType: "desktopPath",
      })
    ).resolves.toBe(true);

    expect(api.getFileSize).toHaveBeenCalledWith("/tmp/book.xlsx");
    expect(api.addRecentFileWithThumbnail).toHaveBeenCalledWith(
      "/tmp/book.xlsx",
      "book.xlsx",
      42,
      "desktopPath",
      undefined
    );
  });

  it("returns false instead of throwing when metadata update fails", async () => {
    const api = await import("@/api");
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    vi.mocked(api.getFileSize).mockRejectedValue(new Error("metadata unavailable"));
    const { tryAddRecentFileWithThumbnail } = await import("@/utils/recentFileTracking");

    await expect(
      tryAddRecentFileWithThumbnail({
        path: "/tmp/book.xlsx",
        fileName: "book.xlsx",
        storageType: "desktopPath",
      })
    ).resolves.toBe(false);

    expect(warn).toHaveBeenCalled();
    expect(api.addRecentFileWithThumbnail).not.toHaveBeenCalled();
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

  it("keeps storage type resolution failures out of the caller's main flow", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const { tryAddRecentFileWithResolvedStorage } = await import("@/utils/recentFileTracking");

    await expect(
      tryAddRecentFileWithResolvedStorage(
        {
          path: "/tmp/book.xlsx",
          fileName: "book.xlsx",
        },
        async () => {
          throw new Error("platform unavailable");
        }
      )
    ).resolves.toBe(false);

    expect(warn).toHaveBeenCalled();
  });
});
