import { beforeEach, describe, expect, it } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { useDocumentSessionStore } from "@/stores/documentSession";
import { useDocumentStatusStore } from "@/stores/documentStatus";
import { useEditorSelectionStore } from "@/stores/editorSelection";
import { usePendingCellSavesStore } from "@/stores/pendingCellSaves";
import { useSearchSessionStore } from "@/stores/searchSession";
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

function queuePendingDraft(value = "draft") {
  const statusStore = useDocumentStatusStore();
  const pendingStore = usePendingCellSavesStore();
  statusStore.markPendingContentChange();
  pendingStore.applyDraft(
    "0,0,0",
    { sheetIndex: 0, row: 0, col: 0, value, oldValue: text("old") },
    text("old")
  );
  return { statusStore, pendingStore };
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

  it("moves the current selection with row and column structure patches", () => {
    const store = useDocumentSessionStore();
    const selectionStore = useEditorSelectionStore();
    store.openDocument({
      path: "/tmp/book.xlsx",
      fileName: "book.xlsx",
      sheets: [sheet("Sheet1", [
        [text("A1"), text("B1"), text("C1")],
        [text("A2"), text("B2"), text("C2")],
        [text("A3"), text("B3"), text("C3")],
      ])],
    });
    selectionStore.selectCell(2, 2);

    store.applyMutationResponse(response({
      revision: 1,
      patches: [{
        type: "RowDeleted",
        data: {
          patch: {
            sheetIndex: 0,
            rowIndex: 0,
            count: 1,
            metadata: {
              merges: [],
              rich: { scope: { type: "rows", start: 0 }, projection: defaultRichProjection() },
            },
          },
        },
      }],
    }));

    expect(selectionStore.selectedCell).toEqual({ row: 1, col: 2 });

    store.applyMutationResponse(response({
      revision: 2,
      patches: [{
        type: "ColumnInserted",
        data: {
          patch: {
            sheetIndex: 0,
            colIndex: 1,
            values: [text("inserted"), text("inserted")],
            metadata: {
              merges: [],
              rich: { scope: { type: "columns", start: 1 }, projection: defaultRichProjection() },
            },
          },
        },
      }],
    }));

    expect(selectionStore.selectedCell).toEqual({ row: 1, col: 3 });
  });

  it("clears the current selection when a structure patch deletes it", () => {
    const store = useDocumentSessionStore();
    const selectionStore = useEditorSelectionStore();
    store.openDocument({
      path: "/tmp/book.xlsx",
      fileName: "book.xlsx",
      sheets: [sheet("Sheet1", [[text("A1"), text("B1")]])],
    });
    selectionStore.selectCell(0, 1);

    store.applyMutationResponse(response({
      revision: 1,
      patches: [{
        type: "ColumnDeleted",
        data: {
          patch: {
            sheetIndex: 0,
            colIndex: 1,
            count: 1,
            metadata: {
              merges: [],
              rich: { scope: { type: "columns", start: 1 }, projection: defaultRichProjection() },
            },
          },
        },
      }],
    }));

    expect(selectionStore.selectedCell).toBeNull();
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

  it("returns command context only for the active document id", () => {
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
        editorState: editorState(),
      },
    }, data.path);

    expect(store.commandContextForDocument(42)).toEqual({
      documentId: 42,
      baseRevision: 7,
    });
    expect(store.commandContextForDocument(99)).toBeNull();
  });

  it("skips queued document mutations after the active document changes", async () => {
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

    let releaseFirstMutation!: () => void;
    const firstMutationStarted = new Promise<void>((resolve) => {
      void store.enqueueDocumentMutation(1, async () => {
        resolve();
        await new Promise<void>((release) => {
          releaseFirstMutation = release;
        });
      });
    });
    await firstMutationStarted;

    let staleMutationRan = false;
    const staleMutation = store.enqueueDocumentMutation(1, async () => {
      staleMutationRan = true;
    });

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

    releaseFirstMutation();
    await staleMutation;

    expect(staleMutationRan).toBe(false);
    expect(store.documentId).toBe(2);
    expect(store.data?.fileName).toBe("next.xlsx");
  });

  it("ignores saved responses for a stale document context", () => {
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

    const applied = store.applySavedDocumentResponseForContext(
      staleContext,
      {
        fileData: {
          ...oldData,
          sheets: [sheet("Sheet1", [[text("saved-old")]])],
        },
        editorSession: {
          documentId: 1,
          revision: 0,
          formulaStatus: readyFormulaStatus(),
          capabilities: defaultWorkbookCapabilities(),
          editorState: editorState(),
        },
      },
      oldData.path
    );

    expect(applied).toBe(false);
    expect(store.documentId).toBe(2);
    expect(store.currentFilePath).toBe(nextData.path);
    expect(store.data?.sheets[0].rows[0][0]).toEqual(text("next"));
  });

  it("saved document responses clear stale search UI state", () => {
    const store = useDocumentSessionStore();
    const searchStore = useSearchSessionStore();
    const data: FileData = {
      path: "/tmp/book.xlsx",
      fileName: "book.xlsx",
      sheets: [sheet("Sheet1", [[text("old")]])],
    };
    store.openDocumentResponse({
      fileData: data,
      editorSession: {
        documentId: 1,
        revision: 0,
        formulaStatus: readyFormulaStatus(),
        capabilities: defaultWorkbookCapabilities(),
        editorState: editorState(),
      },
    }, data.path);
    const requestId = searchStore.beginSearch("old");
    searchStore.applySearchResults(requestId, [{
      sheetIndex: 0,
      sheetName: "Sheet1",
      row: 0,
      col: 0,
      value: "old",
      cellPosition: "A1",
    }]);

    store.applySavedDocumentResponse({
      fileData: {
        path: "/tmp/book.csv",
        fileName: "book.csv",
        sheets: [sheet("Sheet1", [[text("saved")]])],
      },
      editorSession: {
        documentId: 1,
        revision: 1,
        formulaStatus: readyFormulaStatus(),
        capabilities: defaultWorkbookCapabilities(),
        editorState: editorState(),
      },
    }, "/tmp/book.csv");

    expect(store.currentFilePath).toBe("/tmp/book.csv");
    expect(searchStore.searchQuery).toBe("");
    expect(searchStore.searchResults).toEqual([]);
    expect(searchStore.isSearching).toBe(false);
  });

  it("discardPendingLocalWork clears queued drafts and pending dirty state", () => {
    const store = useDocumentSessionStore();
    store.openDocument({
      path: "/tmp/book.xlsx",
      fileName: "book.xlsx",
      sheets: [sheet("Sheet1", [[text("old")]])],
    });
    const { statusStore, pendingStore } = queuePendingDraft();

    store.discardPendingLocalWork();

    expect(statusStore.hasPendingContentChange).toBe(false);
    expect(pendingStore.hasPendingWork()).toBe(false);
    expect(pendingStore.draftCellValues.size).toBe(0);
    expect(pendingStore.queuedCellSaves.size).toBe(0);
  });

  it("openDocumentResponse clears local work from the previous document", () => {
    const store = useDocumentSessionStore();
    store.openDocumentResponse({
      fileData: {
        path: "/tmp/old.xlsx",
        fileName: "old.xlsx",
        sheets: [sheet("Sheet1", [[text("old")]])],
      },
      editorSession: {
        documentId: 1,
        revision: 0,
        formulaStatus: readyFormulaStatus(),
        capabilities: defaultWorkbookCapabilities(),
        editorState: editorState(),
      },
    }, "/tmp/old.xlsx");
    const { statusStore, pendingStore } = queuePendingDraft();

    store.openDocumentResponse({
      fileData: {
        path: "/tmp/next.xlsx",
        fileName: "next.xlsx",
        sheets: [sheet("Sheet1", [[text("next")]])],
      },
      editorSession: {
        documentId: 2,
        revision: 0,
        formulaStatus: readyFormulaStatus(),
        capabilities: defaultWorkbookCapabilities(),
        editorState: editorState(),
      },
    }, "/tmp/next.xlsx");

    expect(store.documentId).toBe(2);
    expect(store.data?.fileName).toBe("next.xlsx");
    expect(statusStore.hasPendingContentChange).toBe(false);
    expect(pendingStore.hasPendingWork()).toBe(false);
    expect(pendingStore.draftCellValues.size).toBe(0);
    expect(pendingStore.queuedCellSaves.size).toBe(0);
  });

  it("openDocument clears local work from the previous backend document", () => {
    const store = useDocumentSessionStore();
    store.openDocumentResponse({
      fileData: {
        path: "/tmp/old.xlsx",
        fileName: "old.xlsx",
        sheets: [sheet("Sheet1", [[text("old")]])],
      },
      editorSession: {
        documentId: 1,
        revision: 0,
        formulaStatus: readyFormulaStatus(),
        capabilities: defaultWorkbookCapabilities(),
        editorState: editorState(),
      },
    }, "/tmp/old.xlsx");
    const { statusStore, pendingStore } = queuePendingDraft();

    store.openDocument({
      path: "",
      fileName: "untitled.xlsx",
      sheets: [sheet("Sheet1", [[text("blank")]])],
    }, null);

    expect(store.documentId).toBeNull();
    expect(store.data?.fileName).toBe("untitled.xlsx");
    expect(statusStore.hasPendingContentChange).toBe(false);
    expect(pendingStore.hasPendingWork()).toBe(false);
    expect(pendingStore.draftCellValues.size).toBe(0);
    expect(pendingStore.queuedCellSaves.size).toBe(0);
  });

  it("clearDocument clears local drafts and queued saves", () => {
    const store = useDocumentSessionStore();
    store.openDocument({
      path: "/tmp/book.xlsx",
      fileName: "book.xlsx",
      sheets: [sheet("Sheet1", [[text("old")]])],
    });
    const { statusStore, pendingStore } = queuePendingDraft();

    store.clearDocument();

    expect(store.data).toBeNull();
    expect(statusStore.hasPendingContentChange).toBe(false);
    expect(pendingStore.hasPendingWork()).toBe(false);
    expect(pendingStore.draftCellValues.size).toBe(0);
    expect(pendingStore.queuedCellSaves.size).toBe(0);
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
