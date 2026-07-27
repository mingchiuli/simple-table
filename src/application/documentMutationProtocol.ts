import type {
  EditorCommandContext,
  MutationCommandContext,
  U64String,
} from '@/types/documentRuntime';
import type {
  EditorMutationResponse,
  MutationResultLookup,
  OpenDocumentResponse,
} from '@/types/protocol';
import { invokeIdempotently } from '@/application/idempotentCommandProtocol';
import { compareU64 } from '@/utils/u64';

type MutationAction = (
  context: MutationCommandContext
) => Promise<EditorMutationResponse>;

export type MutationExecutionResult =
  | { status: 'response'; response: EditorMutationResponse }
  | { status: 'recovered' };

export type DocumentMutationTransport = {
  getMutationResult: (
    documentId: U64String,
    commandId: string
  ) => Promise<MutationResultLookup>;
  getActiveDocument: () => Promise<OpenDocumentResponse | null>;
  getCurrentDocumentProjection: (
    context: EditorCommandContext,
    preferredSheetIndex: number
  ) => Promise<OpenDocumentResponse>;
};

export type DocumentMutationRecovery = {
  preferredSheetIndex: () => number;
  recoverProjection: (
    response: OpenDocumentResponse,
    preferredSheetIndex: number
  ) => boolean;
};

type ProtocolClock = {
  now: () => number;
  sleep: (milliseconds: number) => Promise<void>;
};

type DocumentMutationProtocolOptions = {
  transport: DocumentMutationTransport;
  recovery: DocumentMutationRecovery;
  createCommandId?: () => string;
  clock?: ProtocolClock;
  reportError?: (message: string, error: unknown) => void;
};

const MUTATION_RESULT_POLL_DEADLINE_MS = 3_000;
const MUTATION_RESULT_INITIAL_POLL_INTERVAL_MS = 25;
const MUTATION_RESULT_MAX_POLL_INTERVAL_MS = 250;

export function createDocumentMutationProtocol({
  transport,
  recovery,
  createCommandId = defaultCommandId,
  clock = systemClock,
  reportError = () => undefined,
}: DocumentMutationProtocolOptions) {
  async function execute(
    action: MutationAction,
    context: EditorCommandContext
  ): Promise<MutationExecutionResult> {
    const mutationContext = { ...context, commandId: createCommandId() };
    const invocation = await invokeIdempotently({
      operationId: mutationContext.commandId,
      invoke: () => action(mutationContext),
    });
    if (invocation.status === 'response') {
      return { status: 'response', response: invocation.response };
    }

    try {
      const replay = await waitForMutationResult(context.documentId, mutationContext.commandId);
      if (replay) return { status: 'response', response: replay };
    } catch (error) {
      reportError('Failed to query an ambiguous mutation result', error);
    }

    try {
      const active = await transport.getActiveDocument();
      if (
        active?.editorSession.documentId === context.documentId
        && compareU64(active.editorSession.revision, context.baseRevision) > 0
      ) {
        const preferredSheetIndex = recovery.preferredSheetIndex();
        const projection = await transport.getCurrentDocumentProjection(
          {
            documentId: active.editorSession.documentId,
            baseRevision: active.editorSession.revision,
          },
          preferredSheetIndex
        );
        if (recovery.recoverProjection(projection, preferredSheetIndex)) {
          return { status: 'recovered' };
        }
      }
    } catch (error) {
      reportError('Failed to recover an ambiguous mutation result', error);
    }
    throw invocation.error;
  }

  async function waitForMutationResult(
    documentId: U64String,
    commandId: string
  ): Promise<EditorMutationResponse | null> {
    const deadline = clock.now() + MUTATION_RESULT_POLL_DEADLINE_MS;
    let pollInterval = MUTATION_RESULT_INITIAL_POLL_INTERVAL_MS;
    while (true) {
      const lookup = await transport.getMutationResult(documentId, commandId);
      if (lookup.status === 'completed') {
        if (!lookup.response) {
          throw new Error('Completed mutation lookup did not include a response');
        }
        return lookup.response;
      }
      if (lookup.status === 'missing' || clock.now() >= deadline) {
        return null;
      }
      await clock.sleep(pollInterval);
      pollInterval = Math.min(
        pollInterval * 2,
        MUTATION_RESULT_MAX_POLL_INTERVAL_MS
      );
    }
  }

  return { execute };
}

const systemClock: ProtocolClock = {
  now: () => Date.now(),
  sleep: (milliseconds) => new Promise((resolve) => setTimeout(resolve, milliseconds)),
};

let fallbackCommandId = 0;

function defaultCommandId(): string {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
    return crypto.randomUUID();
  }
  fallbackCommandId += 1;
  return `mutation-${Date.now()}-${fallbackCommandId}`;
}
