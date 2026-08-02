import type {
  DocumentProjection,
  DocumentSessionLifecycle,
  EditorCommandContext,
  U64String,
} from '@/types/documentRuntime';
import type {
  EditorMutationResponse,
  EditorSessionInfo,
  OpenDocumentResponse,
  SavedDocumentResponse,
} from '@/types/protocol';
import { isNextU64 } from '@/utils/u64';
import {
  createDocumentSessionRuntime,
  type DocumentMutationLease,
} from '@/application/documentSessionRuntime';
import {
  editorSessionIdentity,
  hasSupportedMutationProtocol,
  interpretMutationResponse,
  openSessionState,
  recoveredSessionState,
  responseProjection,
  savedSessionState,
  savedSessionStateForContext,
  staleMutationIdentity,
  type DocumentProtocolState,
} from '@/application/documentSessionProtocol';
import {
  editorSessionStatusState,
  editorStatusState,
  selectionTransforms,
} from '@/application/editorRuntimeProtocol';
import type {
  DocumentIdentityStateInput,
  DocumentMutationStateInput,
  DocumentSessionStateInput,
} from '@/types/documentRuntime';
import type {
  DocumentStatusStateInput,
  SelectionTransform,
} from '@/types/editorRuntime';
import type { PreparedOpenDocument } from '@/types/fileRuntime';

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
    readonly currentFilePath: string | null;
    readonly documentId: U64String | null;
    readonly revision: U64String;
    readonly lifecycle: DocumentSessionLifecycle;
    readonly editorCommandDepth: number;
    readonly projectionStale: boolean;
    beginLifecycle(lifecycle: Exclude<DocumentSessionLifecycle, 'idle'>): boolean;
    endLifecycle(lifecycle: Exclude<DocumentSessionLifecycle, 'idle'>): void;
    beginEditorCommand(): boolean;
    endEditorCommand(): void;
    replaceSessionState(state: DocumentSessionStateInput): void;
    replaceAdmittedSessionState(
      state: DocumentSessionStateInput,
      manifestResidentBytes: number,
    ): void;
    replaceProjection(data: DocumentProjection, protectedSheetIndex?: number): void;
    clearDocument(): void;
    applyMutationState(
      state: DocumentMutationStateInput,
      preferredSheetIndex?: number,
    ): MutationApplyResult;
    matchesCommandContext(context: EditorCommandContext): boolean;
    markProjectionStale(identity: DocumentIdentityStateInput): void;
    currentCommandContext(): EditorCommandContext | null;
    commandContextForDocument(documentId: U64String): EditorCommandContext | null;
    applyEditorSessionIdentity(identity: DocumentIdentityStateInput, revisionAdvanced: boolean): void;
    captureSessionSnapshot(): DocumentSnapshot;
    restoreSessionSnapshot(snapshot: DocumentSnapshot): void;
  };
  status: {
    clearPendingContentChange(): void;
    reset(): void;
    applyStatusState(state: DocumentStatusStateInput | null | undefined): void;
    captureSnapshot(): StatusSnapshot;
    restoreSnapshot(snapshot: StatusSnapshot): void;
  };
  selection: {
    reset(): void;
    clearSelection(): void;
    applySelectionTransforms(transforms: SelectionTransform[]): void;
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
    document.replaceSessionState(openSessionState(response, path));
    pending.reset();
    selection.reset();
    search.reset();
    status.reset();
    status.applyStatusState(editorSessionStatusState(response.editorSession));
  }

  function openPreparedDocument(
    prepared: PreparedOpenDocument,
    path: string | null = null,
  ) {
    sessionRuntime.reset();
    regions.reset();
    document.replaceAdmittedSessionState(
      {
        ...prepared.preview.session,
        currentFilePath: path ?? prepared.preview.session.currentFilePath,
      },
      prepared.preview.manifestResidentBytes,
    );
    pending.reset();
    selection.reset();
    search.reset();
    status.reset();
    status.applyStatusState(prepared.preview.status);
  }

  function recoverActiveDocumentResponse(
    response: OpenDocumentResponse,
    preferredSheetIndex = 0
  ): boolean {
    const state = recoveredSessionState(protocolState(), response, preferredSheetIndex);
    if (!state) return false;
    document.replaceSessionState(state);
    regions.reset();
    status.applyStatusState(editorSessionStatusState(response.editorSession));
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
    document.replaceSessionState(
      savedSessionState(protocolState(), response, path, preferredSheetIndex),
    );
    pending.reset();
    status.clearPendingContentChange();
    status.applyStatusState(editorSessionStatusState(response.editorSession));
    clampSelectionToProjection();
    search.reset();
  }

  function applySavedDocumentResponseForContext(
    context: EditorCommandContext,
    response: SavedDocumentResponse,
    path: string | null = null,
    preferredSheetIndex = 0
  ): boolean {
    const state = savedSessionStateForContext(
      protocolState(),
      context,
      response,
      path,
      preferredSheetIndex,
    );
    if (!state) return false;
    document.replaceSessionState(state);
    sessionRuntime.reset();
    regions.reset();
    pending.reset();
    status.clearPendingContentChange();
    status.applyStatusState(editorSessionStatusState(response.editorSession));
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
    const interpretation = interpretMutationResponse(protocolState(), response);
    if (interpretation.status === 'ignored') {
      return { data: document.data, resyncRequired: false, applied: false };
    }
    const result = document.applyMutationState(interpretation.state, preferredSheetIndex);
    if (!result.applied) return result;

    applyResponseStatus(response);
    const projectionAdvanced = isNextU64(response.revision, previousRevision);
    if (projectionAdvanced) regions.reset();
    if (projectionAdvanced) {
      selection.applySelectionTransforms(selectionTransforms(response.patches));
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
      document.replaceProjection(responseProjection(projection), preferredSheetIndex);
      regions.reset();
      status.applyStatusState(editorSessionStatusState(projection.editorSession));
      clampSelectionToProjection();
    } catch (error) {
      if (document.matchesCommandContext(resyncContext)) {
        restoreSnapshot(snapshot);
        regions.reset();
        markProjectionStaleFromMutationResponse(response);
      }
      throw error;
    }
    return { data: document.data, resyncRequired: true, applied: true };
  }

  function markProjectionStaleFromMutationResponse(response: EditorMutationResponse): boolean {
    const identity = staleMutationIdentity(protocolState(), response);
    if (!identity) return false;
    document.markProjectionStale(identity);
    regions.reset();
    if (hasSupportedMutationProtocol(response)) applyResponseStatus(response);
    search.clearSearch();
    return true;
  }

  function markProjectionOutcomeUnknown(context: EditorCommandContext): boolean {
    if (!document.matchesCommandContext(context)) return false;
    document.markProjectionStale({
      documentId: context.documentId,
      revision: context.baseRevision,
    });
    regions.reset();
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
      document.replaceProjection(responseProjection(projection), preferredSheetIndex);
      regions.reset();
      status.applyStatusState(editorSessionStatusState(projection.editorSession));
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
    const result = editorSessionIdentity(protocolState(), info);
    if (!result) return;
    document.applyEditorSessionIdentity(result.state, result.revisionAdvanced);
    status.applyStatusState(editorSessionStatusState(info));
    if (result.revisionAdvanced) {
      regions.reset();
      search.clearSearch();
    }
  }

  function applyResponseStatus(response: EditorMutationResponse) {
    status.applyStatusState(
      editorStatusState(response.editorState, response.formulaStatus, response.capabilities),
    );
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

  function protocolState(): DocumentProtocolState {
    return {
      data: document.data,
      currentFilePath: document.currentFilePath,
      documentId: document.documentId,
      revision: document.revision,
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
    task: (context: EditorCommandContext, lease: DocumentMutationLease) => Promise<T>,
  ): Promise<T | undefined> {
    return sessionRuntime.enqueueMutation(async (lease) => {
      if (document.projectionStale) {
        throw new Error('Document projection is stale; refresh the document before editing.');
      }
      const context = document.commandContextForDocument(documentId);
      return context && lease.isCurrent() ? task(context, lease) : undefined;
    });
  }

  function waitForMutations() {
    return sessionRuntime.waitForMutations();
  }

  return {
    discardPendingLocalWork,
    openDocumentResponse,
    openPreparedDocument,
    recoverActiveDocumentResponse,
    applySavedDocumentResponse,
    applySavedDocumentResponseForContext,
    clearDocument,
    applyMutationResponseWithResync,
    markProjectionStaleFromMutationResponse,
    markProjectionOutcomeUnknown,
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
