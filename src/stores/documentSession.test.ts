import { beforeEach, describe, expect, it } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { useDocumentSessionStore } from "@/stores/documentSession";
import {
  defaultRichProjection,
  defaultWorkbookCapabilities,
  readyFormulaStatus,
  type CellValue,
  type EditorMutationResponse,
  type FileData,
  type SheetData,
} from "@/types";

function text(value: string): CellValue {
  return { type: "cell", kind: "text", raw: value, display: value };
}

function sheet(name: string, rows: CellValue[][]): SheetData {
  return { name, rows, merges: [], rich: defaultRichProjection() };
}

function response(partial: Partial<EditorMutationResponse>): EditorMutationResponse {
  return {
    protocolVersion: 1,
    documentId: 1,
    revision: 1,
    formulaStatus: readyFormulaStatus(),
    capabilities: defaultWorkbookCapabilities(),
    editorState: { canUndo: false, canRedo: false, isDirty: false },
    patches: [],
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
        editorState: { canUndo: true, canRedo: false, isDirty: true },
      },
    }, data.path);

    expect(store.documentId).toBe(42);
    expect(store.revision).toBe(7);
    expect(store.currentFilePath).toBe(data.path);
    expect(store.data?.sheets[0].rows[0][0]).toEqual(text("A1"));
  });
});
