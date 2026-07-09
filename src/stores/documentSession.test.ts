import { beforeEach, describe, expect, it } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { useDocumentSessionStore } from "@/stores/documentSession";
import { useDocumentStatusStore } from "@/stores/documentStatus";
import {
  defaultHistoryStatus,
  defaultRichProjection,
  defaultWorkbookCapabilities,
  readyFormulaStatus,
  type CellValue,
  type EditorMutationResponse,
  type EditorStateInfo,
  type FileData,
  type SheetData,
} from "@/types";

function text(value: string): CellValue {
  return { type: "cell", kind: "text", raw: value, display: value };
}

function sheet(name: string, rows: CellValue[][]): SheetData {
  return { name, rows, merges: [], rich: defaultRichProjection() };
}

function editorState(partial: Partial<EditorStateInfo> = {}): EditorStateInfo {
  return {
    canUndo: false,
    canRedo: false,
    isDirty: false,
    history: defaultHistoryStatus(),
    ...partial,
  };
}

function response(partial: Partial<EditorMutationResponse>): EditorMutationResponse {
  return {
    protocolVersion: 1,
    documentId: 1,
    revision: 1,
    formulaStatus: readyFormulaStatus(),
    capabilities: defaultWorkbookCapabilities(),
    editorState: editorState(),
    patches: [],
    searchIndexUpdate: { rebuildAll: false, rebuildSheets: [] },
    ...partial,
  };
}

describe("documentSession store", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
  });

  it("applies SheetUpdated patches in revision order", () => {
    const store = useDocumentSessionStore();
    const data: FileData = {
      path: "/tmp/book.xlsx",
      fileName: "book.xlsx",
      sheets: [sheet("Sheet1", [[text("old")]])],
    };
    store.openDocument(data, data.path);

    const result = store.applyMutationResponse(response({
      revision: 1,
      patches: [
        {
          type: "SheetUpdated",
          data: { patch: { sheetIndex: 0, sheet: sheet("Sheet1", [[text("new")]]) } },
        },
      ],
    }));

    expect(result.resyncRequired).toBe(false);
    expect(store.revision).toBe(1);
    expect(store.data?.sheets[0].rows[0][0]).toEqual(text("new"));
  });

  it("requests resync when a mutation response skips revisions", () => {
    const store = useDocumentSessionStore();
    store.openDocument({
      path: "/tmp/book.xlsx",
      fileName: "book.xlsx",
      sheets: [sheet("Sheet1", [[text("old")]])],
    });

    const result = store.applyMutationResponse(response({ revision: 3 }));

    expect(result.resyncRequired).toBe(true);
    expect(store.revision).toBe(3);
  });

  it("loads a fresh projection when applying a response that requires resync", async () => {
    const store = useDocumentSessionStore();
    const current: FileData = {
      path: "/tmp/book.xlsx",
      fileName: "book.xlsx",
      sheets: [sheet("Sheet1", [[text("old")]])],
    };
    const fresh: FileData = {
      path: "/tmp/book.xlsx",
      fileName: "book.xlsx",
      sheets: [sheet("Sheet1", [[text("fresh")]])],
    };
    store.openDocumentResponse({
      fileData: current,
      editorSession: {
        documentId: 1,
        revision: 0,
        formulaStatus: readyFormulaStatus(),
        capabilities: defaultWorkbookCapabilities(),
        editorState: editorState(),
      },
    }, current.path);

    const result = await store.applyMutationResponseWithResync(
      response({ revision: 3 }),
      async (context) => {
        expect(context).toEqual({ documentId: 1, baseRevision: 3 });
        return fresh;
      }
    );

    expect(result.resyncRequired).toBe(true);
    expect(store.revision).toBe(3);
    expect(store.data?.sheets[0].rows[0][0]).toEqual(text("fresh"));
  });

  it("rolls back session state when required resync projection loading fails", async () => {
    const store = useDocumentSessionStore();
    const statusStore = useDocumentStatusStore();
    const current: FileData = {
      path: "/tmp/book.xlsx",
      fileName: "book.xlsx",
      sheets: [sheet("Sheet1", [[text("old")]])],
    };
    store.openDocumentResponse({
      fileData: current,
      editorSession: {
        documentId: 1,
        revision: 0,
        formulaStatus: readyFormulaStatus(),
        capabilities: defaultWorkbookCapabilities(),
        editorState: editorState(),
      },
    }, current.path);

    await expect(
      store.applyMutationResponseWithResync(
        response({
          revision: 3,
          editorState: editorState({ isDirty: true }),
        }),
        async (context) => {
          expect(context).toEqual({ documentId: 1, baseRevision: 3 });
          throw new Error("projection unavailable");
        }
      )
    ).rejects.toThrow("projection unavailable");

    expect(store.documentId).toBe(1);
    expect(store.revision).toBe(0);
    expect(store.data?.sheets[0].rows[0][0]).toEqual(text("old"));
    expect(statusStore.isContentDirty).toBe(false);
  });

  it("does not restore an old document if the session changes while resync fails", async () => {
    const store = useDocumentSessionStore();
    const oldData: FileData = {
      path: "/tmp/old.xlsx",
      fileName: "old.xlsx",
      sheets: [sheet("Sheet1", [[text("old")]])],
    };
    const nextData: FileData = {
      path: "/tmp/next.xlsx",
      fileName: "next.xlsx",
      sheets: [sheet("Sheet1", [[text("next")]])],
    };
    store.openDocumentResponse({
      fileData: oldData,
      editorSession: {
        documentId: 1,
        revision: 0,
        formulaStatus: readyFormulaStatus(),
        capabilities: defaultWorkbookCapabilities(),
        editorState: editorState(),
      },
    }, oldData.path);

    await expect(
      store.applyMutationResponseWithResync(
        response({ revision: 3 }),
        async () => {
          store.openDocumentResponse({
            fileData: nextData,
            editorSession: {
              documentId: 2,
              revision: 0,
              formulaStatus: readyFormulaStatus(),
              capabilities: defaultWorkbookCapabilities(),
              editorState: editorState(),
            },
          }, nextData.path);
          throw new Error("projection unavailable");
        }
      )
    ).rejects.toThrow("projection unavailable");

    expect(store.documentId).toBe(2);
    expect(store.revision).toBe(0);
    expect(store.data?.fileName).toBe("next.xlsx");
    expect(store.data?.sheets[0].rows[0][0]).toEqual(text("next"));
  });

  it("does not apply mutation failure session refresh when projection refresh fails", async () => {
    const store = useDocumentSessionStore();
    const statusStore = useDocumentStatusStore();
    const current: FileData = {
      path: "/tmp/book.xlsx",
      fileName: "book.xlsx",
      sheets: [sheet("Sheet1", [[text("old")]])],
    };
    store.openDocumentResponse({
      fileData: current,
      editorSession: {
        documentId: 1,
        revision: 0,
        formulaStatus: readyFormulaStatus(),
        capabilities: defaultWorkbookCapabilities(),
        editorState: editorState(),
      },
    }, current.path);

    await expect(
      store.refreshAfterMutationFailure(
        async (context) => {
          expect(context).toEqual({ documentId: 1, baseRevision: 0 });
          return {
            documentId: 1,
            revision: 3,
            formulaStatus: readyFormulaStatus(),
            capabilities: defaultWorkbookCapabilities(),
            editorState: editorState({ isDirty: true }),
          };
        },
        async (context) => {
          expect(context).toEqual({ documentId: 1, baseRevision: 0 });
          throw new Error("projection unavailable");
        }
      )
    ).rejects.toThrow("projection unavailable");

    expect(store.documentId).toBe(1);
    expect(store.revision).toBe(0);
    expect(store.data?.sheets[0].rows[0][0]).toEqual(text("old"));
    expect(statusStore.isContentDirty).toBe(false);
  });

  it("accepts status-only responses at the current revision", () => {
    const store = useDocumentSessionStore();
    store.openDocument({
      path: "/tmp/book.xlsx",
      fileName: "book.xlsx",
      sheets: [sheet("Sheet1", [[text("current")]])],
    });
    store.applyMutationResponse(response({ revision: 1 }));

    const result = store.applyMutationResponse(response({
      revision: 1,
      patches: [],
      editorState: editorState({ isDirty: true }),
    }));

    expect(result.resyncRequired).toBe(false);
    expect(store.data?.sheets[0].rows[0][0]).toEqual(text("current"));
  });

  it("ignores stale responses from an older revision", () => {
    const store = useDocumentSessionStore();
    store.openDocument({
      path: "/tmp/book.xlsx",
      fileName: "book.xlsx",
      sheets: [sheet("Sheet1", [[text("current")]])],
    });
    store.applyMutationResponse(response({ revision: 2 }));

    const result = store.applyMutationResponse(response({
      revision: 1,
      patches: [
        {
          type: "SheetUpdated",
          data: { patch: { sheetIndex: 0, sheet: sheet("Sheet1", [[text("stale")]]) } },
        },
      ],
    }));

    expect(result.resyncRequired).toBe(false);
    expect(store.data?.sheets[0].rows[0][0]).toEqual(text("current"));
  });

  it("ignores mutation responses after the document has been cleared", () => {
    const store = useDocumentSessionStore();

    const result = store.applyMutationResponse(response({
      documentId: 99,
      revision: 1,
      patches: [
        {
          type: "SheetUpdated",
          data: { patch: { sheetIndex: 0, sheet: sheet("Stale", [[text("stale")]]) } },
        },
      ],
    }));

    expect(result.resyncRequired).toBe(false);
    expect(store.documentId).toBeNull();
    expect(store.revision).toBe(0);
    expect(store.data).toBeNull();
  });

  it("does not clear busy state when an overlapping lifecycle is rejected", () => {
    const store = useDocumentSessionStore();

    expect(store.beginLifecycle("saving")).toBe(true);
    expect(store.beginLifecycle("loading")).toBe(false);

    store.endLifecycle("loading");

    expect(store.lifecycle).toBe("saving");
  });

  it("opens a document response with backend session identity", () => {
    const store = useDocumentSessionStore();
    const data: FileData = {
      path: "/tmp/opened.xlsx",
      fileName: "opened.xlsx",
      sheets: [sheet("Sheet1", [[text("A1")]])],
    };

    store.openDocumentResponse({
      fileData: data,
      editorSession: {
        documentId: 42,
        revision: 7,
        formulaStatus: readyFormulaStatus(),
        capabilities: defaultWorkbookCapabilities(),
        editorState: editorState({ canUndo: true, isDirty: true }),
      },
    }, data.path);

    expect(store.documentId).toBe(42);
    expect(store.revision).toBe(7);
    expect(store.currentFilePath).toBe(data.path);
    expect(store.data?.sheets[0].rows[0][0]).toEqual(text("A1"));
  });

  it("clears the frontend session when backend session is unavailable", () => {
    const store = useDocumentSessionStore();
    const statusStore = useDocumentStatusStore();
    const data: FileData = {
      path: "/tmp/opened.xlsx",
      fileName: "opened.xlsx",
      sheets: [sheet("Sheet1", [[text("A1")]])],
    };

    store.openDocumentResponse({
      fileData: data,
      editorSession: {
        documentId: 42,
        revision: 7,
        formulaStatus: readyFormulaStatus(),
        capabilities: defaultWorkbookCapabilities(),
        editorState: editorState({ canUndo: true, isDirty: true }),
      },
    }, data.path);
    statusStore.markPendingContentChange();

    store.applyEditorSession(null);

    expect(store.data).toBeNull();
    expect(store.currentFilePath).toBeNull();
    expect(store.documentId).toBeNull();
    expect(store.revision).toBe(0);
    expect(statusStore.canUndo).toBe(false);
    expect(statusStore.isContentDirty).toBe(false);
    expect(statusStore.hasPendingContentChange).toBe(false);
    expect(statusStore.formulaStatus).toEqual(readyFormulaStatus());
  });

  it("does not clear a new document when a stale context refresh returns empty", () => {
    const store = useDocumentSessionStore();
    const oldData: FileData = {
      path: "/tmp/old.xlsx",
      fileName: "old.xlsx",
      sheets: [sheet("Sheet1", [[text("old")]])],
    };
    const nextData: FileData = {
      path: "/tmp/next.xlsx",
      fileName: "next.xlsx",
      sheets: [sheet("Sheet1", [[text("next")]])],
    };

    store.openDocumentResponse({
      fileData: oldData,
      editorSession: {
        documentId: 1,
        revision: 0,
        formulaStatus: readyFormulaStatus(),
        capabilities: defaultWorkbookCapabilities(),
        editorState: editorState(),
      },
    }, oldData.path);
    const staleContext = store.requireCommandContext();

    store.openDocumentResponse({
      fileData: nextData,
      editorSession: {
        documentId: 2,
        revision: 0,
        formulaStatus: readyFormulaStatus(),
        capabilities: defaultWorkbookCapabilities(),
        editorState: editorState(),
      },
    }, nextData.path);
    store.applyEditorSessionForContext(staleContext, null);

    expect(store.documentId).toBe(2);
    expect(store.data?.fileName).toBe("next.xlsx");
  });

  it("does not adopt backend session identity without a loaded projection", () => {
    const store = useDocumentSessionStore();

    store.applyEditorSessionForContext(null, {
      documentId: 42,
      revision: 7,
      formulaStatus: readyFormulaStatus(),
      capabilities: defaultWorkbookCapabilities(),
      editorState: editorState({ canUndo: true, isDirty: true }),
    });

    expect(store.data).toBeNull();
    expect(store.documentId).toBeNull();
    expect(store.revision).toBe(0);
  });

  it("clearDocument resets status owned by the active document", () => {
    const store = useDocumentSessionStore();
    const statusStore = useDocumentStatusStore();
    store.openDocument({
      path: "/tmp/book.xlsx",
      fileName: "book.xlsx",
      sheets: [sheet("Sheet1", [[text("old")]])],
    });
    statusStore.applyEditorState(editorState({ canUndo: true, isDirty: true }));
    statusStore.markPendingContentChange();

    store.clearDocument();

    expect(store.data).toBeNull();
    expect(statusStore.canUndo).toBe(false);
    expect(statusStore.isContentDirty).toBe(false);
    expect(statusStore.hasPendingContentChange).toBe(false);
  });

  it("does not allow overlapping document lifecycle actions", () => {
    const store = useDocumentSessionStore();

    expect(store.beginLifecycle("saving")).toBe(true);
    expect(store.lifecycle).toBe("saving");
    expect(store.beginLifecycle("loading")).toBe(false);
    expect(store.lifecycle).toBe("saving");

    store.endLifecycle("loading");
    expect(store.lifecycle).toBe("saving");
    store.endLifecycle("saving");
    expect(store.lifecycle).toBe("idle");
  });
});
