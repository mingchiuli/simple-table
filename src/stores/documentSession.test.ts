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
    store.openDocument(current, current.path);

    const result = await store.applyMutationResponseWithResync(
      response({ revision: 3 }),
      async () => fresh
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
    store.openDocument(current, current.path);

    await expect(
      store.applyMutationResponseWithResync(
        response({
          revision: 3,
          editorState: editorState({ isDirty: true }),
        }),
        async () => {
          throw new Error("projection unavailable");
        }
      )
    ).rejects.toThrow("projection unavailable");

    expect(store.documentId).toBeNull();
    expect(store.revision).toBe(0);
    expect(store.data?.sheets[0].rows[0][0]).toEqual(text("old"));
    expect(statusStore.isContentDirty).toBe(false);
  });

  it("does not apply mutation failure session refresh when projection refresh fails", async () => {
    const store = useDocumentSessionStore();
    const statusStore = useDocumentStatusStore();
    const current: FileData = {
      path: "/tmp/book.xlsx",
      fileName: "book.xlsx",
      sheets: [sheet("Sheet1", [[text("old")]])],
    };
    store.openDocument(current, current.path);

    await expect(
      store.refreshAfterMutationFailure(
        async () => ({
          documentId: 1,
          revision: 3,
          formulaStatus: readyFormulaStatus(),
          capabilities: defaultWorkbookCapabilities(),
          editorState: editorState({ isDirty: true }),
        }),
        async () => {
          throw new Error("projection unavailable");
        }
      )
    ).rejects.toThrow("projection unavailable");

    expect(store.documentId).toBeNull();
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
});
