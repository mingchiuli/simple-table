import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  type PreparedOpenDocument,
} from "@/types";

function mockDesktopPlatform(fileOps: Record<string, unknown>) {
  vi.doMock("@/platform/runtime", () => ({
    getPlatform: () => "macos",
  }));
  vi.doMock("@/platform/desktop", () => ({
    desktopAPI: {
      fileOps: {
        pickOpenFile: vi.fn(),
        prepareOpenFile: vi.fn(),
        saveFile: vi.fn(),
        ...fileOps,
      },
    },
  }));
}

function preparedOpen(): PreparedOpenDocument {
  return { token: "prepared-recent" } as PreparedOpenDocument;
}

describe("platform loader", () => {
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
  });

  it("propagates open-selection discard failures to the caller", async () => {
    const discardOpenFileSelection = vi.fn().mockRejectedValue(new Error("cleanup failed"));
    mockDesktopPlatform({ discardOpenFileSelection });

    const platform = await import("@/platform/loader");

    await expect(
      platform.discardOpenFileSelection({
        path: "/tmp/imported.xlsx",
        fileName: "imported.xlsx",
      })
    ).rejects.toThrow("cleanup failed");
    expect(discardOpenFileSelection).toHaveBeenCalledWith({
      path: "/tmp/imported.xlsx",
      fileName: "imported.xlsx",
    });
  });

  it("propagates reserved-save discard failures to the caller", async () => {
    const discardSaveLocation = vi.fn().mockRejectedValue(new Error("cleanup failed"));
    mockDesktopPlatform({ discardSaveLocation });

    const platform = await import("@/platform/loader");

    await expect(platform.discardSaveLocation("/tmp/reserved.xlsx")).rejects.toThrow(
      "cleanup failed"
    );
    expect(discardSaveLocation).toHaveBeenCalledWith("/tmp/reserved.xlsx");
  });

  it("uses trusted recent-file preparation when the platform provides it", async () => {
    const prepareOpenFile = vi.fn();
    const prepared = preparedOpen();
    const prepareRecentFile = vi.fn().mockResolvedValue(prepared);
    mockDesktopPlatform({ prepareOpenFile, prepareRecentFile });
    const recent = {
      id: "recent",
      path: "/tmp/recent.xlsx",
      fileName: "recent.xlsx",
      lastOpened: 1,
      fileSize: 2,
      storageType: "desktopPath" as const,
    };

    const platform = await import("@/platform/loader");

    await expect(platform.prepareRecentFile(recent)).resolves.toBe(prepared);
    expect(prepareRecentFile).toHaveBeenCalledWith(recent);
    expect(prepareOpenFile).not.toHaveBeenCalled();
  });

  it("falls back to path preparation without trusted recent-file preparation", async () => {
    const prepared = preparedOpen();
    const prepareOpenFile = vi.fn().mockResolvedValue(prepared);
    mockDesktopPlatform({ prepareOpenFile });

    const platform = await import("@/platform/loader");

    await expect(
      platform.prepareRecentFile({
        id: "recent",
        path: "/tmp/recent.xlsx",
        fileName: "recent.xlsx",
        lastOpened: 1,
        fileSize: 2,
        storageType: "desktopPath",
      })
    ).resolves.toBe(prepared);
    expect(prepareOpenFile).toHaveBeenCalledWith("/tmp/recent.xlsx");
  });
});
