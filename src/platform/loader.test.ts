import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  defaultHistoryStatus,
  defaultWorkbookCapabilities,
  readyFormulaStatus,
  type OpenDocumentResponse,
} from "@/types";

function mockDesktopPlatform(fileOps: Record<string, unknown>) {
  vi.doMock("@/utils/platform", () => ({
    getPlatform: () => "macos",
  }));
  vi.doMock("@/platform/desktop", () => ({
    desktopAPI: {
      fileOps: {
        pickOpenFile: vi.fn(),
        readFile: vi.fn(),
        saveFile: vi.fn(),
        ...fileOps,
      },
    },
  }));
}

function openedResponse(): OpenDocumentResponse {
  return {
    fileData: {
      path: "/tmp/recent.xlsx",
      fileName: "recent.xlsx",
      sheets: [],
    },
    editorSession: {
      documentId: 1,
      revision: 0,
      formulaStatus: readyFormulaStatus(),
      capabilities: defaultWorkbookCapabilities(),
      editorState: {
        canUndo: false,
        canRedo: false,
        isDirty: false,
        history: defaultHistoryStatus(),
      },
    },
  };
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

  it("uses trusted recent-file reads when the platform provides them", async () => {
    const readFile = vi.fn();
    const opened = openedResponse();
    const readRecentFile = vi.fn().mockResolvedValue(opened);
    mockDesktopPlatform({ readFile, readRecentFile });
    const recent = {
      id: "recent",
      path: "/tmp/recent.xlsx",
      fileName: "recent.xlsx",
      lastOpened: 1,
      fileSize: 2,
      storageType: "desktopPath" as const,
    };

    const platform = await import("@/platform/loader");

    await expect(platform.readRecentFile(recent)).resolves.toBe(opened);
    expect(readRecentFile).toHaveBeenCalledWith(recent);
    expect(readFile).not.toHaveBeenCalled();
  });

  it("falls back to path reads for platforms without trusted recent-file reads", async () => {
    const opened = openedResponse();
    const readFile = vi.fn().mockResolvedValue(opened);
    mockDesktopPlatform({ readFile });

    const platform = await import("@/platform/loader");

    await expect(
      platform.readRecentFile({
        id: "recent",
        path: "/tmp/recent.xlsx",
        fileName: "recent.xlsx",
        lastOpened: 1,
        fileSize: 2,
        storageType: "desktopPath",
      })
    ).resolves.toBe(opened);
    expect(readFile).toHaveBeenCalledWith("/tmp/recent.xlsx");
  });
});
