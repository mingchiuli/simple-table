import {
  interpretMutationResponse,
  openSessionState,
  type DocumentProtocolState,
} from '@/application/documentSessionProtocol';
import type { EditorMutationResponse, OpenDocumentResponse } from '@/types/protocol';
import { createDocumentWorkspaceRuntime } from '@/composables/documentWorkspaceRuntime';
import { useDocumentSessionStore } from '@/stores/documentSession';

type DocumentSessionStore = ReturnType<typeof useDocumentSessionStore>;

export function openDocumentSession(
  store: DocumentSessionStore,
  response: OpenDocumentResponse,
  path: string | null = null,
) {
  documentRuntime(store).documentSession.replaceSessionState(openSessionState(response, path));
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
  return documentRuntime(store).documentSession.applyMutationState(
    interpretation.state,
    protectedSheetIndex,
  );
}

export function documentRegionCache(store: DocumentSessionStore) {
  return documentRuntime(store).regionCache;
}

function documentRuntime(store: DocumentSessionStore) {
  const runtime = createDocumentWorkspaceRuntime();
  if (runtime.document !== store) throw new Error('Document store is owned by another runtime');
  return runtime;
}

function protocolState(store: DocumentSessionStore): DocumentProtocolState {
  return {
    data: store.data,
    currentFilePath: store.currentFilePath,
    documentId: store.documentId,
    revision: store.revision,
  };
}
