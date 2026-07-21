import { applyProjectionPatches, createDocumentProjection } from '@/projection/documentProjection';
import {
  runtimeDocumentManifest,
  runtimeEditorPatches,
  runtimeRegionProjection,
  runtimeSheetExtents,
} from '@/application/documentProjectionProtocol';
import { EDITOR_MUTATION_PROTOCOL_VERSION } from '@/types/protocol';
import type {
  EditorCommandContext,
  EditorMutationResponse,
  EditorSessionInfo,
  OpenDocumentResponse,
  SavedDocumentResponse,
} from '@/types/protocol';
import type {
  DocumentIdentityStateInput,
  DocumentMutationStateInput,
  DocumentProjection,
  DocumentSessionStateInput,
  U64String,
} from '@/types/documentRuntime';
import { compareU64, isNextU64, maxU64 } from '@/utils/u64';

export type DocumentProtocolState = {
  data: DocumentProjection | null;
  currentFilePath: string | null;
  documentId: U64String | null;
  revision: U64String;
};

export type MutationInterpretation =
  | { status: 'ignored' }
  | { status: 'accepted'; state: DocumentMutationStateInput };

export function openSessionState(
  response: OpenDocumentResponse,
  path: string | null = null,
  preferredSheetIndex = response.initialRegion?.region.sheetIndex ?? 0,
): DocumentSessionStateInput {
  return {
    data: createDocumentProjection(
      runtimeDocumentManifest(response.document),
      response.initialRegion ? runtimeRegionProjection(response.initialRegion) : undefined,
    ),
    currentFilePath: path !== null ? path : response.document.path || null,
    documentId: response.editorSession.documentId,
    revision: response.editorSession.revision,
    preferredSheetIndex,
    activatePreferredSheet: false,
    resetEditorCommandDepth: true,
    preserveResidentSheetOrder: false,
  };
}

export function recoveredSessionState(
  current: DocumentProtocolState,
  response: OpenDocumentResponse,
  preferredSheetIndex = 0,
): DocumentSessionStateInput | null {
  if (
    current.documentId !== response.editorSession.documentId
    || compareU64(response.editorSession.revision, current.revision) < 0
  ) return null;
  return {
    data: createDocumentProjection(
      runtimeDocumentManifest(response.document),
      response.initialRegion ? runtimeRegionProjection(response.initialRegion) : undefined,
    ),
    currentFilePath: response.document.path || current.currentFilePath,
    documentId: response.editorSession.documentId,
    revision: response.editorSession.revision,
    preferredSheetIndex,
    activatePreferredSheet: false,
    resetEditorCommandDepth: false,
    preserveResidentSheetOrder: false,
  };
}

export function savedSessionState(
  current: DocumentProtocolState,
  response: SavedDocumentResponse,
  path: string | null = null,
  preferredSheetIndex = 0,
): DocumentSessionStateInput {
  if (!response.document && (!response.identity || !current.data)) {
    throw new Error('Saved document response did not include manifest or identity data');
  }
  const data = response.document
    ? createDocumentProjection(runtimeDocumentManifest(response.document))
    : {
        ...current.data!,
        path: response.identity!.path,
        fileName: response.identity!.fileName,
      };
  const selected = Math.min(preferredSheetIndex, Math.max(0, data.sheets.length - 1));
  const responsePath = response.document?.path ?? response.identity?.path;
  return {
    data,
    currentFilePath: path !== null ? path : responsePath || null,
    documentId: response.editorSession.documentId,
    revision: response.editorSession.revision,
    preferredSheetIndex: selected,
    activatePreferredSheet: response.document !== undefined,
    resetEditorCommandDepth: false,
    preserveResidentSheetOrder: response.document === undefined,
  };
}

export function savedSessionStateForContext(
  current: DocumentProtocolState,
  context: EditorCommandContext,
  response: SavedDocumentResponse,
  path: string | null = null,
  preferredSheetIndex = 0,
): DocumentSessionStateInput | null {
  if (
    response.editorSession.documentId !== context.documentId
    || compareU64(response.editorSession.revision, context.baseRevision) < 0
    || current.documentId !== context.documentId
    || current.revision !== context.baseRevision
  ) return null;
  return savedSessionState(current, response, path, preferredSheetIndex);
}

export function interpretMutationResponse(
  current: DocumentProtocolState,
  response: EditorMutationResponse,
): MutationInterpretation {
  if (response.protocolVersion !== EDITOR_MUTATION_PROTOCOL_VERSION) {
    throw new Error(`Unsupported editor mutation protocol: ${response.protocolVersion}`);
  }
  if (current.documentId !== null && response.documentId !== current.documentId) {
    return { status: 'ignored' };
  }
  if (current.documentId === null && current.data === null) return { status: 'ignored' };
  if (compareU64(response.revision, current.revision) < 0) return { status: 'ignored' };

  if (
    compareU64(response.revision, current.revision) > 0
    && !isNextU64(response.revision, current.revision)
  ) {
    return acceptedMutation(current.data, response, true);
  }
  if (response.revision === current.revision) {
    return response.patches?.length
      ? acceptedMutation(current.data, response, true)
      : acceptedMutation(current.data, response, false);
  }

  const projection = applyProjectionPatches(
    current.data,
    runtimeEditorPatches(response.patches),
    runtimeSheetExtents(response.sheetExtents),
  );
  return acceptedMutation(projection.data, response, projection.resyncRequired);
}

export function responseProjection(response: OpenDocumentResponse): DocumentProjection {
  return createDocumentProjection(
    runtimeDocumentManifest(response.document),
    response.initialRegion ? runtimeRegionProjection(response.initialRegion) : undefined,
  );
}

export function staleMutationIdentity(
  current: DocumentProtocolState,
  response: EditorMutationResponse,
): DocumentIdentityStateInput | null {
  if (current.documentId !== null && response.documentId !== current.documentId) return null;
  if (current.documentId === null && current.data === null) return null;
  if (compareU64(response.revision, current.revision) < 0) return null;
  return { documentId: response.documentId, revision: response.revision };
}

export function editorSessionIdentity(
  current: DocumentProtocolState,
  info: EditorSessionInfo,
): { state: DocumentIdentityStateInput; revisionAdvanced: boolean } | null {
  if (current.data === null) return null;
  if (current.documentId !== null && info.documentId !== current.documentId) return null;
  return {
    state: { documentId: info.documentId, revision: maxU64(current.revision, info.revision) },
    revisionAdvanced: compareU64(info.revision, current.revision) > 0,
  };
}

export function hasSupportedMutationProtocol(response: EditorMutationResponse): boolean {
  return response.protocolVersion === EDITOR_MUTATION_PROTOCOL_VERSION;
}

function acceptedMutation(
  data: DocumentProjection | null,
  response: EditorMutationResponse,
  resyncRequired: boolean,
): MutationInterpretation {
  return {
    status: 'accepted',
    state: {
      data,
      documentId: response.documentId,
      revision: response.revision,
      resyncRequired,
    },
  };
}
