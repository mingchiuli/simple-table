import type { createDocumentRegionCache } from '@/application/documentRegionCache';
import type {
  DocumentIdentityStateInput,
  DocumentMutationStateInput,
  DocumentProjection,
  DocumentSessionLifecycle,
  DocumentSessionStateInput,
  EditorCommandContext,
  U64String,
} from '@/types/documentRuntime';
import type { useDocumentSessionStore } from '@/stores/documentSession';

type DocumentSessionStore = ReturnType<typeof useDocumentSessionStore>;
type DocumentRegionCache = ReturnType<typeof createDocumentRegionCache>;

export function createDocumentSessionStoreAdapter(
  document: DocumentSessionStore,
  regionCache: DocumentRegionCache,
) {
  function replaceSessionState(state: DocumentSessionStateInput) {
    if (!state.preserveResidentSheetOrder) regionCache.reset();
    document.replaceSessionState(state);
    regionCache.reconcileProjection(state.preferredSheetIndex);
    if (state.activatePreferredSheet && document.data?.sheets[state.preferredSheetIndex]) {
      regionCache.activateResidentSheet(state.preferredSheetIndex, state.preferredSheetIndex);
    }
  }

  function replaceProjection(data: DocumentProjection, protectedSheetIndex = 0) {
    regionCache.reset();
    document.replaceProjection(data);
    regionCache.reconcileProjection(protectedSheetIndex);
  }

  function applyMutationState(
    state: DocumentMutationStateInput,
    protectedSheetIndex = 0,
  ) {
    const result = document.applyMutationState(state);
    regionCache.reconcileProjection(protectedSheetIndex);
    return result;
  }

  function clearDocument() {
    regionCache.reset();
    document.clearDocument();
  }

  function captureSessionSnapshot() {
    return {
      document: document.captureSessionSnapshot(),
      regions: regionCache.captureSnapshot(),
    };
  }

  function restoreSessionSnapshot(snapshot: ReturnType<typeof captureSessionSnapshot>) {
    document.restoreSessionSnapshot(snapshot.document);
    regionCache.restoreSnapshot(snapshot.regions);
  }

  return {
    get data() { return document.data; },
    get currentFilePath() { return document.currentFilePath; },
    get documentId() { return document.documentId; },
    get revision() { return document.revision; },
    get lifecycle() { return document.lifecycle; },
    get editorCommandDepth() { return document.editorCommandDepth; },
    get projectionStale() { return document.projectionStale; },
    beginLifecycle: (lifecycle: Exclude<DocumentSessionLifecycle, 'idle'>) =>
      document.beginLifecycle(lifecycle),
    endLifecycle: (lifecycle: Exclude<DocumentSessionLifecycle, 'idle'>) =>
      document.endLifecycle(lifecycle),
    beginEditorCommand: () => document.beginEditorCommand(),
    endEditorCommand: () => document.endEditorCommand(),
    replaceSessionState,
    replaceProjection,
    clearDocument,
    applyMutationState,
    matchesCommandContext: (context: EditorCommandContext) =>
      document.matchesCommandContext(context),
    markProjectionStale: (identity: DocumentIdentityStateInput) =>
      document.markProjectionStale(identity),
    currentCommandContext: (): EditorCommandContext | null => document.currentCommandContext(),
    commandContextForDocument: (documentId: U64String) =>
      document.commandContextForDocument(documentId),
    applyEditorSessionIdentity: (
      identity: DocumentIdentityStateInput,
      revisionAdvanced: boolean,
    ) => document.applyEditorSessionIdentity(identity, revisionAdvanced),
    captureSessionSnapshot,
    restoreSessionSnapshot,
  };
}
