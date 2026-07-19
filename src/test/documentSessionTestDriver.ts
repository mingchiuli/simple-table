import {
  interpretMutationResponse,
  openSessionState,
  type DocumentProtocolState,
} from '@/application/documentSessionProtocol';
import type { EditorMutationResponse, OpenDocumentResponse } from '@/types/generated';
import { useDocumentSessionStore } from '@/stores/documentSession';

type DocumentSessionStore = ReturnType<typeof useDocumentSessionStore>;

export function openDocumentSession(
  store: DocumentSessionStore,
  response: OpenDocumentResponse,
  path: string | null = null,
) {
  store.replaceSessionState(openSessionState(response, path));
}

export function applyDocumentMutation(
  store: DocumentSessionStore,
  response: EditorMutationResponse,
  protectedSheetIndex = 0,
) {
  const interpretation = interpretMutationResponse(protocolState(store), response);
  if (interpretation.status === 'ignored') {
    return { data: store.data, resyncRequired: false, applied: false };
  }
  return store.applyMutationState(interpretation.state, protectedSheetIndex);
}

function protocolState(store: DocumentSessionStore): DocumentProtocolState {
  return {
    data: store.data,
    currentFilePath: store.currentFilePath,
    documentId: store.documentId,
    revision: store.revision,
  };
}
