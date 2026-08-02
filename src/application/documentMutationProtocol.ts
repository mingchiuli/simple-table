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
import {
  isOperationOutcomeUnknown,
  OperationOutcomeUnknownError,
  runBeforeDeadline,
} from '@/application/operationOutcome';

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
  markOutcomeUnknown?: (context: EditorCommandContext) => void;
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
  responseTimeoutMs?: number;
  lookupTimeoutMs?: number;
  terminalResultTimeoutMs?: number;
};

const MUTATION_RESULT_POLL_DEADLINE_MS = 3_000;
const MUTATION_RESULT_INITIAL_POLL_INTERVAL_MS = 25;
const MUTATION_RESULT_MAX_POLL_INTERVAL_MS = 250;
const DEFAULT_RESPONSE_TIMEOUT_MS = 30_000;
const DEFAULT_LOOKUP_TIMEOUT_MS = 10_000;
const DEFAULT_TERMINAL_RESULT_TIMEOUT_MS = 120_000;

export function createDocumentMutationProtocol({
  transport,
  recovery,
  createCommandId = defaultCommandId,
  clock = systemClock,
  reportError = () => undefined,
  cancellation = neverCancelled,
  responseTimeoutMs = DEFAULT_RESPONSE_TIMEOUT_MS,
  lookupTimeoutMs = DEFAULT_LOOKUP_TIMEOUT_MS,
  terminalResultTimeoutMs = DEFAULT_TERMINAL_RESULT_TIMEOUT_MS,
}: DocumentMutationProtocolOptions) {
  async function execute(
    action: MutationAction,
    context: EditorCommandContext
  ): Promise<MutationExecutionResult> {
    const mutationContext = { ...context, commandId: createCommandId() };
    const outcomeUnknown = () => new OperationOutcomeUnknownError(
      'mutation',
      mutationContext.commandId,
    );
    try {
      return await executeMutation(action, context, mutationContext, outcomeUnknown);
    } catch (error) {
      if (isOperationOutcomeUnknown(error)) {
        try {
          recovery.markOutcomeUnknown?.(context);
        } catch (callbackError) {
          reportError('Failed to mark a mutation with an unknown outcome', callbackError);
        }
      }
      throw error;
    }
  }

  async function executeMutation(
    action: MutationAction,
    context: EditorCommandContext,
    mutationContext: MutationCommandContext,
    outcomeUnknown: () => OperationOutcomeUnknownError,
  ): Promise<MutationExecutionResult> {
    const invocation = await raceWithOperationCancellation(
      () => invokeIdempotently({
        operationId: mutationContext.commandId,
        invoke: () => action(mutationContext),
        responseTimeoutMs,
        timeoutError: outcomeUnknown,
        cancellation,
      }),
      cancellation,
    );
    if (invocation.status === 'response') {
      return { status: 'response', response: invocation.response };
    }

    let replay: Awaited<ReturnType<typeof waitForMutationResult>> = { status: 'missing' };
    try {
      replay = await waitForMutationResult(
        context.documentId,
        mutationContext.commandId,
        outcomeUnknown,
      );
    } catch (error) {
      if (isOperationCancelled(error)) throw error;
      if (isOperationOutcomeUnknown(error)) throw error;
      reportError('Failed to query an ambiguous mutation result', error);
    }
    if (replay.status === 'completed') {
      return { status: 'response', response: replay.response };
    }
    if (replay.status === 'failed') throw replay.error;

    try {
      const active = await runBeforeDeadline(
        () => transport.getActiveDocument(),
        lookupTimeoutMs,
        outcomeUnknown,
        cancellation,
      );
      if (
        active?.editorSession.documentId === context.documentId
        && compareU64(active.editorSession.revision, context.baseRevision) > 0
      ) {
        const preferredSheetIndex = recovery.preferredSheetIndex();
        const projection = await runBeforeDeadline(
          () => transport.getCurrentDocumentProjection(
            {
              documentId: active.editorSession.documentId,
              baseRevision: active.editorSession.revision,
            },
            preferredSheetIndex
          ),
          lookupTimeoutMs,
          outcomeUnknown,
          cancellation,
        );
        recovery.recoverProjection(projection, preferredSheetIndex);
      }
    } catch (error) {
      if (isOperationCancelled(error)) throw error;
      if (isOperationOutcomeUnknown(error)) throw error;
      reportError('Failed to recover an ambiguous mutation result', error);
    }
    throw invocation.error;
  }

  async function waitForMutationResult(
    documentId: U64String,
    commandId: string,
    outcomeUnknown: () => OperationOutcomeUnknownError,
  ): Promise<
    | { status: 'completed'; response: EditorMutationResponse }
    | { status: 'failed'; error: { code: string; message: string } }
    | { status: 'missing' }
  > {
    const discoveryDeadline = clock.now() + MUTATION_RESULT_POLL_DEADLINE_MS;
    let pollInterval = MUTATION_RESULT_INITIAL_POLL_INTERVAL_MS;
    let observedPending = false;
    let terminalDeadline: number | null = null;
    while (true) {
      const lookup = await runBeforeDeadline(
        () => transport.getMutationResult(documentId, commandId),
        lookupTimeoutMs,
        outcomeUnknown,
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
        terminalDeadline ??= clock.now() + terminalResultTimeoutMs;
        if (clock.now() >= terminalDeadline) throw outcomeUnknown();
      } else if (observedPending) {
        throw new Error('Pending mutation result disappeared before reaching a terminal state');
      } else if (clock.now() >= discoveryDeadline) {
        return { status: 'missing' };
      }
      await raceWithOperationCancellation(() => clock.sleep(pollInterval), cancellation);
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
