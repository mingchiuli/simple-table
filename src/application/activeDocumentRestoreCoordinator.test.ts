import { beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { restoreActiveDocument } from "@/application/activeDocumentRestoreCoordinator";
import { useDocumentSessionStore } from "@/stores/documentSession";
import type { OpenDocumentResponse } from '@/types/protocol';
import { defaultWorkbookCapabilities, readyFormulaStatus } from "@/types";
import { openResponseFromFileData } from "@/test/documentFixtures";
import { openDocumentSession } from '@/test/documentSessionTestDriver';
import {
  createDocumentWorkspaceTestContext,
  type DocumentWorkspaceTestContext,
} from '@/test/documentWorkspaceTestContext';

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

describe("active document restore coordinator", () => {
  let workspace: DocumentWorkspaceTestContext;

  beforeEach(() => {
    setActivePinia(createPinia());
    workspace = createDocumentWorkspaceTestContext();
    vi.resetAllMocks();
  });

  function restore(loadActiveDocument: () => Promise<OpenDocumentResponse | null>) {
    return restoreActiveDocument({
      isFrontendSessionInitialized: () =>
        workspace.runtime.document.data !== null
        || workspace.runtime.document.documentId !== null,
      loadActiveDocument,
      publishActiveDocument: (document) => {
        workspace.runtime.session.openDocumentResponse(
          document,
          document.document.path || null,
        );
      },
    });
  }

  it("hydrates a frontend session from the active backend document", async () => {
    const loadActiveDocument = vi.fn().mockResolvedValue(activeDocument());

    await expect(restore(loadActiveDocument)).resolves.toBe(true);

    const store = useDocumentSessionStore();
    expect(store.documentId).toBe('9');
    expect(store.revision).toBe('4');
    expect(store.currentFilePath).toBe("/tmp/recovered.xlsx");
  });

  it("does not replace an already initialized frontend session", async () => {
    openDocumentSession(workspace.runtime, activeDocument(), "/tmp/recovered.xlsx");
    const loadActiveDocument = vi.fn().mockResolvedValue(null);

    await expect(restore(loadActiveDocument)).resolves.toBe(false);
    expect(loadActiveDocument).not.toHaveBeenCalled();
  });

  it("leaves the frontend empty when the backend has no active document", async () => {
    const loadActiveDocument = vi.fn().mockResolvedValue(null);

    await expect(restore(loadActiveDocument)).resolves.toBe(false);

    const store = useDocumentSessionStore();
    expect(store.data).toBeNull();
    expect(store.documentId).toBeNull();
  });

  it("does not overwrite a frontend session initialized while the backend request is pending", async () => {
    let resolveActiveDocument!: (document: OpenDocumentResponse | null) => void;
    const pending = new Promise<OpenDocumentResponse | null>((resolve) => {
      resolveActiveDocument = resolve;
    });
    const restoring = restore(() => pending);
    const current = activeDocument();
    current.editorSession.documentId = '10';
    openDocumentSession(workspace.runtime, current, "/tmp/current.xlsx");

    resolveActiveDocument(activeDocument());

    await expect(restoring).resolves.toBe(false);
    expect(workspace.runtime.document.documentId).toBe('10');
  });
});
