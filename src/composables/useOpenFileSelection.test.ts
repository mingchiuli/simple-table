import { beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { useOpenFileSelection } from "@/composables/useOpenFileSelection";
import { useDocumentSessionStore } from "@/stores/documentSession";
import {
  defaultHistoryStatus,
  defaultRichProjection,
  defaultWorkbookCapabilities,
  readyFormulaStatus,
  type OpenDocumentResponse,
} from "@/types";
import type { OpenFileSelection } from "@/platform";
import type { DocumentReplacementLease } from "@/composables/useDocumentReplacementGuard";

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

function replacementLease(): DocumentReplacementLease {
  return {
    commit: vi.fn(),
    cancel: vi.fn(),
  };
}

describe("useOpenFileSelection", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
  });

  it("discards the selection when replacement is cancelled", async () => {
    const platform = await import("@/platform");
    const beginDocumentReplacement = vi.fn().mockResolvedValue(null);
    const { openSelectedFileOrDiscard } = useOpenFileSelection({
      beginDocumentReplacement,
    });

    await expect(openSelectedFileOrDiscard(selection)).resolves.toBe(false);

    expect(beginDocumentReplacement).toHaveBeenCalledTimes(1);
    expect(platform.readFile).not.toHaveBeenCalled();
    expect(platform.discardOpenFileSelection).toHaveBeenCalledWith(selection);
  });

  it("discards the selection when file reading fails", async () => {
    const platform = await import("@/platform");
    const replacement = replacementLease();
    const beginDocumentReplacement = vi.fn().mockResolvedValue(replacement);
    const { openSelectedFileOrDiscard } = useOpenFileSelection({
      beginDocumentReplacement,
    });
    vi.mocked(platform.readFile).mockRejectedValue(new Error("broken file"));

    await expect(openSelectedFileOrDiscard(selection)).rejects.toThrow("broken file");

    expect(beginDocumentReplacement).toHaveBeenCalledTimes(1);
    expect(platform.readFile).toHaveBeenCalledWith(selection.path);
    expect(replacement.commit).not.toHaveBeenCalled();
    expect(replacement.cancel).toHaveBeenCalledTimes(1);
    expect(platform.discardOpenFileSelection).toHaveBeenCalledWith(selection);
  });

  it("does not discard a selection after successful open", async () => {
    const platform = await import("@/platform");
    const response = openedResponse();
    const replacement = replacementLease();
    const documentSessionStore = useDocumentSessionStore();
    const { openSelectedFileOrDiscard } = useOpenFileSelection({
      beginDocumentReplacement: vi.fn().mockResolvedValue(replacement),
    });
    vi.mocked(platform.readFile).mockResolvedValue(response);

    await expect(openSelectedFileOrDiscard(selection)).resolves.toBe(true);

    expect(platform.readFile).toHaveBeenCalledWith(selection.path);
    expect(documentSessionStore.documentId).toBe(response.editorSession.documentId);
    expect(documentSessionStore.currentFilePath).toBe(selection.path);
    expect(documentSessionStore.data).toStrictEqual(response.fileData);
    expect(replacement.commit).toHaveBeenCalledTimes(1);
    expect(replacement.cancel).not.toHaveBeenCalled();
    expect(platform.discardOpenFileSelection).not.toHaveBeenCalled();
  });

  it("commits the replacement before publishing the opened document", async () => {
    const platform = await import("@/platform");
    const response = openedResponse();
    const replacement: DocumentReplacementLease = {
      commit: vi.fn(() => {
        expect(useDocumentSessionStore().data).toBeNull();
      }),
      cancel: vi.fn(),
    };
    const documentSessionStore = useDocumentSessionStore();
    const { openSelectedFileOrDiscard } = useOpenFileSelection({
      beginDocumentReplacement: vi.fn().mockResolvedValue(replacement),
    });
    vi.mocked(platform.readFile).mockResolvedValue(response);

    await expect(openSelectedFileOrDiscard(selection)).resolves.toBe(true);

    expect(replacement.commit).toHaveBeenCalledTimes(1);
    expect(replacement.cancel).not.toHaveBeenCalled();
    expect(platform.discardOpenFileSelection).not.toHaveBeenCalled();
    expect(documentSessionStore.data).toStrictEqual(response.fileData);
  });
});
