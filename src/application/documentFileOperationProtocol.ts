import type {
  FileOperationKind,
  FileOperationReceipt,
  FileOperationResultLookup,
} from '@/types/fileRuntime';
import { invokeIdempotently } from '@/application/idempotentCommandProtocol';
import {
  isOperationCancelled,
  neverCancelled,
  raceWithOperationCancellation,
  type OperationCancellationSignal,
} from '@/application/operationCancellation';
import {
  isOperationOutcomeUnknown,
  OperationOutcomeUnknownError,
  runBeforeDeadline,
} from '@/application/operationOutcome';

type ProtocolClock = {
  now: () => number;
  sleep: (milliseconds: number) => Promise<void>;
};

type FileOperationExecution<T> = {
  kind: FileOperationKind;
  invoke: (operationId: string) => Promise<T>;
  receiptForResponse: (response: T) => FileOperationReceipt | null;
  validateReceipt: (receipt: FileOperationReceipt) => boolean;
  recoverResponse: (receipt: FileOperationReceipt) => Promise<T>;
  recoverCancelled?: () => Promise<T> | T;
  recoverAmbiguous?: () => Promise<T | null>;
  onOutcomeUnknown?: (error: OperationOutcomeUnknownError) => void;
};

type DocumentFileOperationProtocolOptions = {
  getFileOperationResult: (operationId: string) => Promise<FileOperationResultLookup>;
  createOperationId?: () => string;
  clock?: ProtocolClock;
  reportError?: (message: string, error: unknown) => void;
  cancellation?: OperationCancellationSignal;
  responseTimeoutMs?: number;
  lookupTimeoutMs?: number;
  terminalResultTimeoutMs?: number;
};

const RESULT_DISCOVERY_DEADLINE_MS = 3_000;
const INITIAL_POLL_INTERVAL_MS = 25;
const MAX_POLL_INTERVAL_MS = 250;
const DEFAULT_RESPONSE_TIMEOUT_MS = 30_000;
const DEFAULT_LOOKUP_TIMEOUT_MS = 10_000;
const DEFAULT_TERMINAL_RESULT_TIMEOUT_MS = 120_000;

export function createDocumentFileOperationProtocol({
  getFileOperationResult,
  createOperationId = defaultOperationId,
  clock = systemClock,
  reportError = () => undefined,
  cancellation = neverCancelled,
  responseTimeoutMs = DEFAULT_RESPONSE_TIMEOUT_MS,
  lookupTimeoutMs = DEFAULT_LOOKUP_TIMEOUT_MS,
  terminalResultTimeoutMs = DEFAULT_TERMINAL_RESULT_TIMEOUT_MS,
}: DocumentFileOperationProtocolOptions) {
  async function execute<T>(operation: FileOperationExecution<T>): Promise<T> {
    const operationId = createOperationId();
    const outcomeUnknown = () => new OperationOutcomeUnknownError(operation.kind, operationId);
    try {
      return await executeOperation(operation, operationId, outcomeUnknown);
    } catch (error) {
      if (isOperationOutcomeUnknown(error)) {
        try {
          operation.onOutcomeUnknown?.(error);
        } catch (callbackError) {
          reportError('Failed to mark an operation with an unknown outcome', callbackError);
        }
      }
      throw error;
    }
  }

  async function executeOperation<T>(
    operation: FileOperationExecution<T>,
    operationId: string,
    outcomeUnknown: () => OperationOutcomeUnknownError,
  ): Promise<T> {
    const invocation = await raceWithOperationCancellation(
      () => invokeIdempotently({
        operationId,
        invoke: operation.invoke,
        responseTimeoutMs,
        timeoutError: outcomeUnknown,
        cancellation,
      }),
      cancellation,
    );
    if (invocation.status === 'response') {
      return admittedResponse(invocation.response, operation);
    }

    let result: Awaited<ReturnType<typeof waitForResult>> = { status: 'missing' };
    try {
      result = await waitForResult(operationId, outcomeUnknown);
      if (result.status === 'completed') {
        const receipt = result.receipt;
        ensureReceipt(receipt, operation);
        return await runBeforeDeadline(
          () => operation.recoverResponse(receipt),
          lookupTimeoutMs,
          outcomeUnknown,
          cancellation,
        );
      }
    } catch (error) {
      if (isOperationCancelled(error)) throw error;
      if (isOperationOutcomeUnknown(error)) throw error;
      reportError('Failed to query an ambiguous file operation result', error);
    }
    if (result.status === 'failed') throw result.error;
    if (result.status === 'cancelled') {
      const recoverCancelled = operation.recoverCancelled;
      if (!recoverCancelled) {
        throw new Error(`${operation.kind} operation unexpectedly reached a cancelled state`);
      }
      return await runBeforeDeadline(
        () => Promise.resolve(recoverCancelled()),
        lookupTimeoutMs,
        outcomeUnknown,
        cancellation,
      );
    }

    const recoverAmbiguous = operation.recoverAmbiguous;
    if (recoverAmbiguous) {
      try {
        const recovered = await runBeforeDeadline(
          () => recoverAmbiguous(),
          lookupTimeoutMs,
          outcomeUnknown,
          cancellation,
        );
        if (recovered) return admittedResponse(recovered, operation);
      } catch (error) {
        if (isOperationCancelled(error)) throw error;
        if (isOperationOutcomeUnknown(error)) throw error;
        reportError('Failed to recover an ambiguous file operation', error);
      }
    }
    throw invocation.error;
  }

  async function waitForResult(
    operationId: string,
    outcomeUnknown: () => OperationOutcomeUnknownError,
  ): Promise<
    | { status: 'completed'; receipt: FileOperationReceipt }
    | { status: 'failed'; error: { code: string; message: string } }
    | { status: 'cancelled' }
    | { status: 'missing' }
  > {
    const discoveryDeadline = clock.now() + RESULT_DISCOVERY_DEADLINE_MS;
    let interval = INITIAL_POLL_INTERVAL_MS;
    let terminalDeadline: number | null = null;
    while (true) {
      const lookup = await runBeforeDeadline(
        () => getFileOperationResult(operationId),
        lookupTimeoutMs,
        outcomeUnknown,
        cancellation,
      );
      if (lookup.status === 'completed') {
        if (!lookup.receipt) {
          throw new Error('Completed file operation lookup did not include a receipt');
        }
        return { status: 'completed', receipt: lookup.receipt };
      }
      if (lookup.status === 'failed') {
        if (!lookup.error) {
          throw new Error('Failed file operation lookup did not include an error');
        }
        return { status: 'failed', error: lookup.error };
      }
      if (lookup.status === 'cancelled') return { status: 'cancelled' };
      if (lookup.status === 'pending') {
        terminalDeadline ??= clock.now() + terminalResultTimeoutMs;
        if (clock.now() >= terminalDeadline) throw outcomeUnknown();
      }
      if (lookup.status === 'missing' && clock.now() >= discoveryDeadline) {
        return { status: 'missing' };
      }
      await raceWithOperationCancellation(() => clock.sleep(interval), cancellation);
      interval = Math.min(interval * 2, MAX_POLL_INTERVAL_MS);
    }
  }

  return { execute };
}

function admittedResponse<T>(response: T, operation: FileOperationExecution<T>): T {
  const receipt = operation.receiptForResponse(response);
  if (!receipt) {
    if (!operation.recoverCancelled) {
      throw new Error(`${operation.kind} operation response did not include a receipt`);
    }
    return response;
  }
  ensureReceipt(receipt, operation);
  return response;
}

function ensureReceipt<T>(
  receipt: FileOperationReceipt,
  operation: FileOperationExecution<T>,
) {
  if (receipt.kind !== operation.kind || !operation.validateReceipt(receipt)) {
    throw new Error(`Backend returned a mismatched ${operation.kind} operation receipt`);
  }
}

const systemClock: ProtocolClock = {
  now: () => Date.now(),
  sleep: (milliseconds) => new Promise((resolve) => setTimeout(resolve, milliseconds)),
};

let fallbackOperationId = 0;

function defaultOperationId(): string {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
    return crypto.randomUUID();
  }
  fallbackOperationId += 1;
  return `file-${Date.now()}-${fallbackOperationId}`;
}
