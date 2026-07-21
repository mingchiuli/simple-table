import { beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import * as api from "@/api";
import { restoreActiveDocument } from "@/composables/restoreActiveDocument";
import { useDocumentSessionStore } from "@/stores/documentSession";
import type { OpenDocumentResponse } from '@/types/protocol';
import { defaultWorkbookCapabilities, readyFormulaStatus } from "@/types";
import { openResponseFromFileData } from "@/test/documentFixtures";
import { openDocumentSession } from '@/test/documentSessionTestDriver';

vi.mock("@/api", () => ({
  getActiveDocument: vi.fn(),
}));

function activeDocument(): OpenDocumentResponse {
  const fileData = {
      path: "/tmp/recovered.xlsx",
      fileName: "recovered.xlsx",
      sheets: [{ name: "Sheet1", rows: [], merges: [], rich: { hasMoreDrawings: false, hasStyleMetadata: false, hasHyperlinks: false, hasFreezePane: false } }],
    };
  const editorSession = {
      documentId: '9' as const,
      revision: '4' as const,
      formulaStatus: readyFormulaStatus(),
      capabilities: defaultWorkbookCapabilities(),
      editorState: {
        canUndo: true,
        canRedo: false,
        isDirty: true,
        history: {
          isTruncated: false,
          undoEntries: 1,
          redoEntries: 0,
          undoEstimatedBytes: 10,
          redoEstimatedBytes: 0,
          maxHistoryBytes: 100,
          maxSingleEntryBytes: 50,
        },
      },
    };
  return openResponseFromFileData(fileData, editorSession);
}

describe("restoreActiveDocument", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.resetAllMocks();
  });

  it("hydrates a frontend session from the active backend document", async () => {
    vi.mocked(api.getActiveDocument).mockResolvedValue(activeDocument());

    await expect(restoreActiveDocument()).resolves.toBe(true);

    const store = useDocumentSessionStore();
    expect(store.documentId).toBe('9');
    expect(store.revision).toBe('4');
    expect(store.currentFilePath).toBe("/tmp/recovered.xlsx");
  });

  it("does not replace an already initialized frontend session", async () => {
    const store = useDocumentSessionStore();
    openDocumentSession(store, activeDocument(), "/tmp/recovered.xlsx");

    await expect(restoreActiveDocument()).resolves.toBe(false);
    expect(api.getActiveDocument).not.toHaveBeenCalled();
  });

  it("leaves the frontend empty when the backend has no active document", async () => {
    vi.mocked(api.getActiveDocument).mockResolvedValue(null);

    await expect(restoreActiveDocument()).resolves.toBe(false);

    const store = useDocumentSessionStore();
    expect(store.data).toBeNull();
    expect(store.documentId).toBeNull();
  });
});
