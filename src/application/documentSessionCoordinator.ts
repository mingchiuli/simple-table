import { useDocumentSessionStore, type MutationApplyResult } from '@/stores/documentSession';
import { useDocumentStatusStore } from '@/stores/documentStatus';
import { useEditorSelectionStore } from '@/stores/editorSelection';
import { usePendingCellSavesStore } from '@/stores/pendingCellSaves';
import { useSearchSessionStore } from '@/stores/searchSession';
import type {
  EditorCommandContext,
  EditorMutationResponse,
  EditorSessionInfo,
  OpenDocumentResponse,
  SavedDocumentResponse,
} from '@/types';
import { isNextU64 } from '@/utils/u64';

type DocumentSessionStore = ReturnType<typeof useDocumentSessionStore>;
type DocumentStatusStore = ReturnType<typeof useDocumentStatusStore>;
type EditorSelectionStore = ReturnType<typeof useEditorSelectionStore>;
type PendingCellSavesStore = ReturnType<typeof usePendingCellSavesStore>;
type SearchSessionStore = ReturnType<typeof useSearchSessionStore>;

export type DocumentSessionCoordinatorPorts = {
  document: DocumentSessionStore;
  status: DocumentStatusStore;
  selection: EditorSelectionStore;
  pending: PendingCellSavesStore;
  search: SearchSessionStore;
};

type FetchProjection = (
  context: EditorCommandContext,
  preferredSheetIndex: number
) => Promise<OpenDocumentResponse>;

type FetchEditorSession = (
  context: EditorCommandContext | null
) => Promise<EditorSessionInfo | null | undefined>;

export function createDocumentSessionCoordinator({
  document,
  status,
  selection,
  pending,
  search,
}: DocumentSessionCoordinatorPorts) {
  function discardPendingLocalWork() {
    document.discardPendingLocalWork();
    pending.reset();
    status.clearPendingContentChange();
  }

  function openDocumentResponse(response: OpenDocumentResponse, path: string | null = null) {
    document.openDocumentResponse(response, path);
    pending.reset();
    selection.reset();
    search.reset();
    status.reset();
    status.applyEditorSession(response.editorSession);
  }

  function recoverActiveDocumentResponse(
    response: OpenDocumentResponse,
    preferredSheetIndex = 0
  ): boolean {
    if (!document.recoverActiveDocumentResponse(response, preferredSheetIndex)) return false;
    status.applyEditorSession(response.editorSession);
    clampSelectionToProjection();
    search.clearSearch();
    return true;
  }

  function applySavedDocumentResponse(
    response: SavedDocumentResponse,
    path: string | null = null,
    preferredSheetIndex = 0
  ) {
    document.applySavedDocumentResponse(response, path, preferredSheetIndex);
    pending.reset();
    status.clearPendingContentChange();
    status.applyEditorSession(response.editorSession);
    clampSelectionToProjection();
    search.reset();
  }

  function applySavedDocumentResponseForContext(
    context: EditorCommandContext,
    response: SavedDocumentResponse,
    path: string | null = null,
    preferredSheetIndex = 0
  ): boolean {
    if (!document.applySavedDocumentResponseForContext(
      context,
      response,
      path,
      preferredSheetIndex
    )) return false;
    pending.reset();
    status.clearPendingContentChange();
    status.applyEditorSession(response.editorSession);
    clampSelectionToProjection();
    search.reset();
    return true;
  }

  function clearDocument() {
    document.clearDocument();
    pending.reset();
    selection.reset();
    search.reset();
    status.reset();
  }

  async function applyMutationResponseWithResync(
    response: EditorMutationResponse,
    fetchProjection: FetchProjection,
    preferredSheetIndex = 0
  ): Promise<MutationApplyResult> {
    const snapshot = captureSnapshot();
    const previousRevision = document.revision;
    const result = document.applyMutationResponse(response, preferredSheetIndex);
    if (!result.applied) return result;

    applyResponseStatus(response);
    const projectionAdvanced = isNextU64(response.revision, previousRevision);
    if (projectionAdvanced) {
      selection.applyEditorPatches(response.patches);
      clampSelectionToProjection();
    }
    if (result.resyncRequired || mutationInvalidatesSearch(response)) {
      search.clearSearch();
    }
    if (!result.resyncRequired) return result;

    const resyncContext = { documentId: response.documentId, baseRevision: response.revision };
    try {
      const projection = await fetchProjection(resyncContext, preferredSheetIndex);
      if (!document.matchesCommandContext(resyncContext)) {
        return { data: document.data, resyncRequired: true, applied: false };
      }
      document.replaceDocumentProjection(projection, preferredSheetIndex);
      status.applyEditorSession(projection.editorSession);
      clampSelectionToProjection();
    } catch (error) {
      if (document.matchesCommandContext(resyncContext)) {
        restoreSnapshot(snapshot);
        document.markProjectionStaleFromMutationResponse(response);
        applyResponseStatus(response);
        search.clearSearch();
      }
      throw error;
    }
    return { data: document.data, resyncRequired: true, applied: true };
  }

  function markProjectionStaleFromMutationResponse(response: EditorMutationResponse): boolean {
    if (!document.markProjectionStaleFromMutationResponse(response)) return false;
    if (response.protocolVersion === 4) applyResponseStatus(response);
    search.clearSearch();
    return true;
  }

  async function refreshAfterMutationFailure(
    fetchEditorSession: FetchEditorSession,
    fetchProjection?: FetchProjection,
    preferredSheetIndex = 0
  ) {
    const context = document.currentCommandContext();
    if (!fetchProjection || !context) {
      applyEditorSessionForContext(context, await fetchEditorSession(context));
      return;
    }

    const snapshot = captureSnapshot();
    try {
      const [projection, session] = await Promise.all([
        fetchProjection(context, preferredSheetIndex),
        fetchEditorSession(context),
      ]);
      if (!document.matchesCommandContext(context)) return;
      document.replaceDocumentProjection(projection, preferredSheetIndex);
      status.applyEditorSession(projection.editorSession);
      clampSelectionToProjection();
      search.clearSearch();
      applyEditorSessionForContext(context, session);
    } catch (error) {
      if (document.matchesCommandContext(context)) restoreSnapshot(snapshot);
      throw error;
    }
  }

  function applyEditorSessionForContext(
    context: EditorCommandContext | null,
    info: EditorSessionInfo | null | undefined
  ) {
    if (context) {
      if (document.matchesCommandContext(context)) applyEditorSession(info);
      return;
    }
    if (document.documentId !== null) return;
    if (!info) {
      clearDocument();
    } else if (document.data !== null) {
      applyEditorSession(info);
    }
  }

  function applyEditorSession(info: EditorSessionInfo | null | undefined) {
    if (!info) {
      clearDocument();
      return;
    }
    const result = document.applyEditorSessionIdentity(info);
    if (!result.applied) return;
    status.applyEditorSession(info);
    if (result.revisionAdvanced) search.clearSearch();
  }

  function applyResponseStatus(response: EditorMutationResponse) {
    status.applyRuntimeStatus(response.formulaStatus, response.capabilities);
    status.applyEditorState(response.editorState);
  }

  function clampSelectionToProjection() {
    if (!document.data) {
      selection.clearSelection();
      return;
    }
    selection.clampToSheetData(
      document.data.sheets.length,
      (sheetIndex, row, col) => {
        const sheet = document.data?.sheets[sheetIndex];
        if (!sheet) return false;
        const extent = sheet.extent;
        return row >= 0 && col >= 0 && row < extent.rowCount && col < extent.columnCount;
      }
    );
  }

  function captureSnapshot() {
    return {
      document: document.captureSessionSnapshot(),
      status: status.captureSnapshot(),
      selection: selection.captureSnapshot(),
      search: search.captureSnapshot(),
    };
  }

  function restoreSnapshot(snapshot: ReturnType<typeof captureSnapshot>) {
    document.restoreSessionSnapshot(snapshot.document);
    status.restoreSnapshot(snapshot.status);
    selection.restoreSnapshot(snapshot.selection);
    search.restoreSnapshot(snapshot.search);
  }

  return {
    discardPendingLocalWork,
    openDocumentResponse,
    recoverActiveDocumentResponse,
    applySavedDocumentResponse,
    applySavedDocumentResponseForContext,
    clearDocument,
    applyMutationResponseWithResync,
    markProjectionStaleFromMutationResponse,
    refreshAfterMutationFailure,
    applyEditorSessionForContext,
  };
}

type DocumentSessionCoordinator = ReturnType<typeof createDocumentSessionCoordinator>;
const documentSessionCoordinators = new WeakMap<object, DocumentSessionCoordinator>();

export function useDocumentSessionCoordinator() {
  const document = useDocumentSessionStore();
  const existing = documentSessionCoordinators.get(document);
  if (existing) return existing;
  const coordinator = createDocumentSessionCoordinator({
    document,
    status: useDocumentStatusStore(),
    selection: useEditorSelectionStore(),
    pending: usePendingCellSavesStore(),
    search: useSearchSessionStore(),
  });
  documentSessionCoordinators.set(document, coordinator);
  return coordinator;
}

function mutationInvalidatesSearch(response: EditorMutationResponse): boolean {
  return (response.patches ?? []).some((patch) => patch.type !== 'Layout');
}
