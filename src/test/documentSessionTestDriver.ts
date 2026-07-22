import {
  interpretMutationResponse,
  openSessionState,
  type DocumentProtocolState,
} from '@/application/documentSessionProtocol';
import type { EditorMutationResponse, OpenDocumentResponse } from '@/types/protocol';
import type { DocumentWorkspaceRuntime } from '@/composables/documentWorkspaceRuntime';

export function openDocumentSession(
  runtime: DocumentWorkspaceRuntime,
  response: OpenDocumentResponse,
  path: string | null = null,
) {
  runtime.documentSession.replaceSessionState(openSessionState(response, path));
}

export function applyDocumentMutation(
  runtime: DocumentWorkspaceRuntime,
  response: EditorMutationResponse,
  protectedSheetIndex = 0,
) {
  const store = runtime.document;
  const interpretation = interpretMutationResponse(protocolState(store), response);
  if (interpretation.status === 'ignored') {
    return { data: store.data, resyncRequired: false, applied: false };
  }
  return runtime.documentSession.applyMutationState(
    interpretation.state,
    protectedSheetIndex,
  );
}

export function documentRegionCache(runtime: DocumentWorkspaceRuntime) {
  return runtime.regionCache;
}

function protocolState(store: DocumentWorkspaceRuntime['document']): DocumentProtocolState {
  return {
    data: store.data,
    currentFilePath: store.currentFilePath,
    documentId: store.documentId,
    revision: store.revision,
  };
}
