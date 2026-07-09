import { beforeEach, describe, expect, it, vi } from "vitest";

function mockDesktopPlatform(fileOps: Record<string, unknown>) {
  vi.doMock("@/utils/platform", () => ({
    getPlatform: () => "macos",
  }));
  vi.doMock("@/platform/desktop", () => ({
    desktopAPI: {
      storageType: "desktopPath",
      fileOps: {
        pickOpenFile: vi.fn(),
        readFile: vi.fn(),
        saveFile: vi.fn(),
        ...fileOps,
      },
    },
  }));
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
});
