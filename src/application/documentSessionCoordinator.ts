import type {
  DocumentProjection,
  DocumentSessionLifecycle,
  EditorCommandContext,
  EditorMutationResponse,
  EditorPatch,
  EditorSessionInfo,
  FormulaStatus,
  OpenDocumentResponse,
  SavedDocumentResponse,
  U64String,
  WorkbookCapabilities,
} from '@/types';
import { EDITOR_MUTATION_PROTOCOL_VERSION } from '@/types';
import { isNextU64 } from '@/utils/u64';
import { createDocumentSessionRuntime } from '@/application/documentSessionRuntime';

export type MutationApplyResult = {
  data: DocumentProjection | null;
  resyncRequired: boolean;
  applied: boolean;
};

export type DocumentSessionCoordinatorPorts<
  DocumentSnapshot,
  StatusSnapshot,
  SelectionSnapshot,
  SearchSnapshot,
> = {
  document: {
    readonly data: DocumentProjection | null;
    readonly documentId: U64String | null;
    readonly revision: U64String;
    readonly lifecycle: DocumentSessionLifecycle;
    readonly editorCommandDepth: number;
    readonly projectionStale: boolean;
    beginLifecycle(lifecycle: Exclude<DocumentSessionLifecycle, 'idle'>): boolean;
    endLifecycle(lifecycle: Exclude<DocumentSessionLifecycle, 'idle'>): void;
    beginEditorCommand(): boolean;
    endEditorCommand(): void;
    openDocumentResponse(response: OpenDocumentResponse, path?: string | null): void;
    recoverActiveDocumentResponse(response: OpenDocumentResponse, preferredSheetIndex?: number): boolean;
    applySavedDocumentResponse(
      response: SavedDocumentResponse,
      path?: string | null,
      preferredSheetIndex?: number,
    ): void;
    applySavedDocumentResponseForContext(
      context: EditorCommandContext,
      response: SavedDocumentResponse,
      path?: string | null,
      preferredSheetIndex?: number,
    ): boolean;
    clearDocument(): void;
    applyMutationResponse(
      response: EditorMutationResponse,
      preferredSheetIndex?: number,
    ): MutationApplyResult;
    matchesCommandContext(context: EditorCommandContext): boolean;
    replaceDocumentProjection(response: OpenDocumentResponse, preferredSheetIndex?: number): void;
    markProjectionStaleFromMutationResponse(response: EditorMutationResponse): boolean;
    currentCommandContext(): EditorCommandContext | null;
    commandContextForDocument(documentId: U64String): EditorCommandContext | null;
    applyEditorSessionIdentity(info: EditorSessionInfo): {
      applied: boolean;
      revisionAdvanced: boolean;
    };
    captureSessionSnapshot(): DocumentSnapshot;
    restoreSessionSnapshot(snapshot: DocumentSnapshot): void;
  };
  status: {
    clearPendingContentChange(): void;
    reset(): void;
    applyEditorSession(info: EditorSessionInfo | null | undefined): void;
    applyRuntimeStatus(formulaStatus: FormulaStatus, capabilities: WorkbookCapabilities): void;
    applyEditorState(state: EditorMutationResponse['editorState']): void;
    captureSnapshot(): StatusSnapshot;
    restoreSnapshot(snapshot: StatusSnapshot): void;
  };
  selection: {
    reset(): void;
    clearSelection(): void;
    applyEditorPatches(patches: EditorPatch[] | undefined): void;
    clampToSheetData(
      sheetCount: number,
      containsCell: (sheetIndex: number, row: number, col: number) => boolean,
    ): void;
    captureSnapshot(): SelectionSnapshot;
    restoreSnapshot(snapshot: SelectionSnapshot): void;
  };
  pending: { reset(): void };
  search: {
    reset(): void;
    clearSearch(): void;
    captureSnapshot(): SearchSnapshot;
    restoreSnapshot(snapshot: SearchSnapshot): void;
  };
  regions: { reset(): void };
};

type FetchProjection = (
  context: EditorCommandContext,
  preferredSheetIndex: number
) => Promise<OpenDocumentResponse>;

type FetchEditorSession = (
  context: EditorCommandContext | null
) => Promise<EditorSessionInfo | null | undefined>;

export function createDocumentSessionCoordinator<
  DocumentSnapshot,
  StatusSnapshot,
  SelectionSnapshot,
  SearchSnapshot,
>({
  document,
  status,
  selection,
  pending,
  search,
  regions,
}: DocumentSessionCoordinatorPorts<
  DocumentSnapshot,
  StatusSnapshot,
  SelectionSnapshot,
  SearchSnapshot
>) {
  const sessionRuntime = createDocumentSessionRuntime(
    document,
    () => document.beginEditorCommand(),
    () => document.endEditorCommand(),
  );

  function discardPendingLocalWork() {
    sessionRuntime.reset();
    regions.reset();
    pending.reset();
    status.clearPendingContentChange();
  }

  function openDocumentResponse(response: OpenDocumentResponse, path: string | null = null) {
    sessionRuntime.reset();
    regions.reset();
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
    regions.reset();
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
    sessionRuntime.reset();
    regions.reset();
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
    sessionRuntime.reset();
    regions.reset();
    pending.reset();
    status.clearPendingContentChange();
    status.applyEditorSession(response.editorSession);
    clampSelectionToProjection();
    search.reset();
    return true;
  }

  function clearDocument() {
    sessionRuntime.reset();
    regions.reset();
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
    if (projectionAdvanced) regions.reset();
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
      regions.reset();
      status.applyEditorSession(projection.editorSession);
      clampSelectionToProjection();
    } catch (error) {
      if (document.matchesCommandContext(resyncContext)) {
        restoreSnapshot(snapshot);
        regions.reset();
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
    regions.reset();
    if (response.protocolVersion === EDITOR_MUTATION_PROTOCOL_VERSION) applyResponseStatus(response);
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
      regions.reset();
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
    if (result.revisionAdvanced) {
      regions.reset();
      search.clearSearch();
    }
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
    regions.reset();
  }

  function beginLifecycle(lifecycle: Exclude<DocumentSessionLifecycle, 'idle'>): boolean {
    return document.beginLifecycle(lifecycle);
  }

  function endLifecycle(lifecycle: Exclude<DocumentSessionLifecycle, 'idle'>) {
    document.endLifecycle(lifecycle);
    sessionRuntime.notifyInteractionChanged();
  }

  function waitForInteractionIdle() {
    return sessionRuntime.waitForInteractionIdle();
  }

  function beginEditorCommand() {
    return sessionRuntime.beginEditorCommandLease();
  }

  function enqueueDocumentMutation<T>(
    documentId: U64String,
    task: (context: EditorCommandContext) => Promise<T>,
  ): Promise<T | undefined> {
    return sessionRuntime.enqueueMutation(async () => {
      if (document.projectionStale) {
        throw new Error('Document projection is stale; refresh the document before editing.');
      }
      const context = document.commandContextForDocument(documentId);
      return context ? task(context) : undefined;
    });
  }

  function waitForMutations() {
    return sessionRuntime.waitForMutations();
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
    beginLifecycle,
    endLifecycle,
    waitForInteractionIdle,
    beginEditorCommand,
    enqueueDocumentMutation,
    waitForMutations,
  };
}

function mutationInvalidatesSearch(response: EditorMutationResponse): boolean {
  return (response.patches ?? []).some((patch) => patch.type !== 'Layout');
}
