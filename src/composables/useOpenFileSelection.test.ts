import { beforeEach, describe, expect, it, vi } from "vitest";
import { useOpenFileSelection } from "@/composables/useOpenFileSelection";
import {
  defaultHistoryStatus,
  defaultRichProjection,
  defaultWorkbookCapabilities,
  readyFormulaStatus,
  type OpenDocumentResponse,
} from "@/types";
import type { OpenFileSelection } from "@/platform";

vi.mock("@/platform", () => ({
  discardOpenFileSelection: vi.fn(),
  readFile: vi.fn(),
}));

const selection: OpenFileSelection = {
  path: "/tmp/imported.xlsx",
  fileName: "imported.xlsx",
  originalPath: "content://picked",
};

function openedResponse(): OpenDocumentResponse {
  return {
    fileData: {
      path: selection.path,
      fileName: selection.fileName,
      sheets: [
        {
          name: "Sheet1",
          rows: [],
          merges: [],
          rich: defaultRichProjection(),
        },
      ],
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

describe("useOpenFileSelection", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("discards the selection when replacement is cancelled", async () => {
    const platform = await import("@/platform");
    const { openSelectedFileOrDiscard } = useOpenFileSelection({
      prepareForDocumentReplacement: vi.fn().mockResolvedValue(false),
    });

    await expect(openSelectedFileOrDiscard(selection)).resolves.toBeNull();

    expect(platform.readFile).not.toHaveBeenCalled();
    expect(platform.discardOpenFileSelection).toHaveBeenCalledWith(selection);
  });

  it("discards the selection when file reading fails", async () => {
    const platform = await import("@/platform");
    const { openSelectedFileOrDiscard } = useOpenFileSelection({
      prepareForDocumentReplacement: vi.fn().mockResolvedValue(true),
    });
    vi.mocked(platform.readFile).mockRejectedValue(new Error("broken file"));

    await expect(openSelectedFileOrDiscard(selection)).rejects.toThrow("broken file");

    expect(platform.readFile).toHaveBeenCalledWith(selection.path);
    expect(platform.discardOpenFileSelection).toHaveBeenCalledWith(selection);
  });

  it("does not discard a selection after successful open", async () => {
    const platform = await import("@/platform");
    const response = openedResponse();
    const { openSelectedFileOrDiscard } = useOpenFileSelection({
      prepareForDocumentReplacement: vi.fn().mockResolvedValue(true),
    });
    vi.mocked(platform.readFile).mockResolvedValue(response);

    await expect(openSelectedFileOrDiscard(selection)).resolves.toBe(response);

    expect(platform.readFile).toHaveBeenCalledWith(selection.path);
    expect(platform.discardOpenFileSelection).not.toHaveBeenCalled();
  });
});
