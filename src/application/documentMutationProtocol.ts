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
import {
  isOperationCancelled,
  neverCancelled,
  raceWithOperationCancellation,
  type OperationCancellationSignal,
} from '@/application/operationCancellation';
import { compareU64 } from '@/utils/u64';

type MutationAction = (
  context: MutationCommandContext
) => Promise<EditorMutationResponse>;

export type MutationExecutionResult =
  { status: 'response'; response: EditorMutationResponse };

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
  cancellation?: OperationCancellationSignal;
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
  cancellation = neverCancelled,
}: DocumentMutationProtocolOptions) {
  async function execute(
    action: MutationAction,
    context: EditorCommandContext
  ): Promise<MutationExecutionResult> {
    const mutationContext = { ...context, commandId: createCommandId() };
    const invocation = await raceWithOperationCancellation(
      invokeIdempotently({
        operationId: mutationContext.commandId,
        invoke: () => action(mutationContext),
      }),
      cancellation,
    );
    if (invocation.status === 'response') {
      return { status: 'response', response: invocation.response };
    }

    let replay: Awaited<ReturnType<typeof waitForMutationResult>> = { status: 'missing' };
    try {
      replay = await waitForMutationResult(context.documentId, mutationContext.commandId);
    } catch (error) {
      if (isOperationCancelled(error)) throw error;
      reportError('Failed to query an ambiguous mutation result', error);
    }
    if (replay.status === 'completed') {
      return { status: 'response', response: replay.response };
    }
    if (replay.status === 'failed') throw replay.error;

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
        recovery.recoverProjection(projection, preferredSheetIndex);
      }
    } catch (error) {
      reportError('Failed to recover an ambiguous mutation result', error);
    }
    throw invocation.error;
  }

  async function waitForMutationResult(
    documentId: U64String,
    commandId: string
  ): Promise<
    | { status: 'completed'; response: EditorMutationResponse }
    | { status: 'failed'; error: { code: string; message: string } }
    | { status: 'missing' }
  > {
    const discoveryDeadline = clock.now() + MUTATION_RESULT_POLL_DEADLINE_MS;
    let pollInterval = MUTATION_RESULT_INITIAL_POLL_INTERVAL_MS;
    let observedPending = false;
    while (true) {
      const lookup = await raceWithOperationCancellation(
        transport.getMutationResult(documentId, commandId),
        cancellation,
      );
      if (lookup.status === 'completed') {
        if (!lookup.response) {
          throw new Error('Completed mutation lookup did not include a response');
        }
        return { status: 'completed', response: lookup.response };
      }
      if (lookup.status === 'failed') {
        if (!lookup.error) {
          throw new Error('Failed mutation lookup did not include an error');
        }
        return { status: 'failed', error: lookup.error };
      }
      if (lookup.status === 'pending') {
        observedPending = true;
      } else if (observedPending) {
        throw new Error('Pending mutation result disappeared before reaching a terminal state');
      } else if (clock.now() >= discoveryDeadline) {
        return { status: 'missing' };
      }
      await raceWithOperationCancellation(clock.sleep(pollInterval), cancellation);
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
