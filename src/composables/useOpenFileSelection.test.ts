import { beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { useOpenFileSelection } from "@/composables/useOpenFileSelection";
import { useDocumentSessionStore } from "@/stores/documentSession";
import { createDocumentProjection } from "@/projection/documentProjection";
import {
  defaultHistoryStatus,
  defaultRichProjection,
  defaultWorkbookCapabilities,
  readyFormulaStatus,
  type OpenDocumentResponse,
} from "@/types";
import type { OpenFileSelection } from "@/platform";
import type { DocumentReplacementLease } from "@/composables/useDocumentReplacementGuard";
import { openResponseFromFileData } from "@/test/documentFixtures";

const openProtocolMocks = vi.hoisted(() => ({
  prepareOpenFile: vi.fn(),
  commitPreparedDocument: vi.fn(),
  abortPreparedDocument: vi.fn(),
}));

vi.mock("@/api", () => ({
  commitPreparedDocument: openProtocolMocks.commitPreparedDocument,
  abortPreparedDocument: openProtocolMocks.abortPreparedDocument,
}));

vi.mock("@/platform", () => ({
  discardOpenFileSelection: vi.fn(),
  prepareOpenFile: openProtocolMocks.prepareOpenFile,
}));

const selection: OpenFileSelection = {
  path: "/tmp/imported.xlsx",
  fileName: "imported.xlsx",
  originalPath: "content://picked",
};

function openedResponse(): OpenDocumentResponse {
  const fileData = {
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
    };
  const editorSession = {
      documentId: '1' as const,
      revision: '0' as const,
      formulaStatus: readyFormulaStatus(),
      capabilities: defaultWorkbookCapabilities(),
      editorState: {
        canUndo: false,
        canRedo: false,
        isDirty: false,
        history: defaultHistoryStatus(),
      },
    };
  return openResponseFromFileData(fileData, editorSession);
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
    expect(platform.prepareOpenFile).not.toHaveBeenCalled();
    expect(platform.discardOpenFileSelection).toHaveBeenCalledWith(selection);
  });

  it("discards the selection when file reading fails", async () => {
    const platform = await import("@/platform");
    const replacement = replacementLease();
    const beginDocumentReplacement = vi.fn().mockResolvedValue(replacement);
    const { openSelectedFileOrDiscard } = useOpenFileSelection({
      beginDocumentReplacement,
    });
    vi.mocked(platform.prepareOpenFile).mockRejectedValue(new Error("broken file"));

    await expect(openSelectedFileOrDiscard(selection)).rejects.toThrow("broken file");

    expect(beginDocumentReplacement).toHaveBeenCalledTimes(1);
    expect(platform.prepareOpenFile).toHaveBeenCalledWith(selection.path);
    expect(replacement.commit).not.toHaveBeenCalled();
    expect(replacement.cancel).toHaveBeenCalledTimes(1);
    expect(platform.discardOpenFileSelection).toHaveBeenCalledWith(selection);
  });

  it("aborts a prepared document when commit fails", async () => {
    const api = await import("@/api");
    const platform = await import("@/platform");
    const replacement = replacementLease();
    const documentSessionStore = useDocumentSessionStore();
    documentSessionStore.openDocumentResponse(openedResponse(), "/tmp/current.xlsx");
    vi.mocked(platform.prepareOpenFile).mockResolvedValue({ token: "prepared-selection" });
    vi.mocked(api.commitPreparedDocument).mockRejectedValue(new Error("context changed"));

    const { openSelectedFileOrDiscard } = useOpenFileSelection({
      beginDocumentReplacement: vi.fn().mockResolvedValue(replacement),
    });

    await expect(openSelectedFileOrDiscard(selection)).rejects.toThrow("context changed");

    expect(api.commitPreparedDocument).toHaveBeenCalledWith("prepared-selection", {
      documentId: '1',
      baseRevision: '0',
    });
    expect(api.abortPreparedDocument).toHaveBeenCalledWith("prepared-selection");
    expect(replacement.commit).not.toHaveBeenCalled();
    expect(replacement.cancel).toHaveBeenCalledTimes(1);
    expect(platform.discardOpenFileSelection).toHaveBeenCalledWith(selection);
    expect(documentSessionStore.currentFilePath).toBe("/tmp/current.xlsx");
  });

  it("keeps the read error when discarding the failed selection also fails", async () => {
    const platform = await import("@/platform");
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => undefined);
    const replacement = replacementLease();
    const beginDocumentReplacement = vi.fn().mockResolvedValue(replacement);
    const { openSelectedFileOrDiscard } = useOpenFileSelection({
      beginDocumentReplacement,
    });
    vi.mocked(platform.prepareOpenFile).mockRejectedValue(new Error("broken file"));
    vi.mocked(platform.discardOpenFileSelection).mockRejectedValueOnce(new Error("cleanup failed"));

    try {
      await expect(openSelectedFileOrDiscard(selection)).rejects.toThrow("broken file");

      expect(platform.discardOpenFileSelection).toHaveBeenCalledWith(selection);
      expect(consoleError).toHaveBeenCalledWith(
        "Failed to discard open file selection after open error:",
        expect.any(Error)
      );
    } finally {
      consoleError.mockRestore();
    }
  });

  it("does not fail a cancelled replacement when discarding the selection fails", async () => {
    const platform = await import("@/platform");
    const consoleWarn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
    const beginDocumentReplacement = vi.fn().mockResolvedValue(null);
    const { openSelectedFileOrDiscard } = useOpenFileSelection({
      beginDocumentReplacement,
    });
    vi.mocked(platform.discardOpenFileSelection).mockRejectedValue(new Error("cleanup failed"));

    try {
      await expect(openSelectedFileOrDiscard(selection)).resolves.toBe(false);

      expect(platform.prepareOpenFile).not.toHaveBeenCalled();
      expect(platform.discardOpenFileSelection).toHaveBeenCalledWith(selection);
      expect(consoleWarn).toHaveBeenCalledWith(
        "Failed to discard unused open file selection:",
        expect.any(Error)
      );
    } finally {
      consoleWarn.mockRestore();
    }
  });

  it("does not discard a selection after successful open", async () => {
    const platform = await import("@/platform");
    const response = openedResponse();
    const replacement = replacementLease();
    const documentSessionStore = useDocumentSessionStore();
    const { openSelectedFileOrDiscard } = useOpenFileSelection({
      beginDocumentReplacement: vi.fn().mockResolvedValue(replacement),
    });
    vi.mocked(platform.prepareOpenFile).mockResolvedValue({ token: "prepared-selection" });
    openProtocolMocks.commitPreparedDocument.mockResolvedValue(response);

    await expect(openSelectedFileOrDiscard(selection)).resolves.toBe(true);

    expect(platform.prepareOpenFile).toHaveBeenCalledWith(selection.path);
    expect(openProtocolMocks.commitPreparedDocument).toHaveBeenCalledWith(
      "prepared-selection",
      null
    );
    expect(documentSessionStore.documentId).toBe(response.editorSession.documentId);
    expect(documentSessionStore.currentFilePath).toBe(selection.path);
    expect(documentSessionStore.data).toStrictEqual(
      createDocumentProjection(response.document, response.initialRegion)
    );
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
    vi.mocked(platform.prepareOpenFile).mockResolvedValue({ token: "prepared-selection" });
    openProtocolMocks.commitPreparedDocument.mockResolvedValue(response);

    await expect(openSelectedFileOrDiscard(selection)).resolves.toBe(true);

    expect(replacement.commit).toHaveBeenCalledTimes(1);
    expect(replacement.cancel).not.toHaveBeenCalled();
    expect(platform.discardOpenFileSelection).not.toHaveBeenCalled();
    expect(documentSessionStore.data).toStrictEqual(
      createDocumentProjection(response.document, response.initialRegion)
    );
  });
});
