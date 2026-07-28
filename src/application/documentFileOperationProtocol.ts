import type {
  FileOperationKind,
  FileOperationReceipt,
  FileOperationResultLookup,
} from '@/types/fileRuntime';
import { invokeIdempotently } from '@/application/idempotentCommandProtocol';

type ProtocolClock = {
  now: () => number;
  sleep: (milliseconds: number) => Promise<void>;
};

type FileOperationExecution<T> = {
  kind: FileOperationKind;
  invoke: (operationId: string) => Promise<T>;
  receiptForResponse: (response: T) => FileOperationReceipt;
  validateReceipt: (receipt: FileOperationReceipt) => boolean;
  recoverResponse: (receipt: FileOperationReceipt) => Promise<T>;
  recoverAmbiguous?: () => Promise<T | null>;
};

type DocumentFileOperationProtocolOptions = {
  getFileOperationResult: (operationId: string) => Promise<FileOperationResultLookup>;
  createOperationId?: () => string;
  clock?: ProtocolClock;
  reportError?: (message: string, error: unknown) => void;
};

const RESULT_DISCOVERY_DEADLINE_MS = 3_000;
const INITIAL_POLL_INTERVAL_MS = 25;
const MAX_POLL_INTERVAL_MS = 250;

export function createDocumentFileOperationProtocol({
  getFileOperationResult,
  createOperationId = defaultOperationId,
  clock = systemClock,
  reportError = () => undefined,
}: DocumentFileOperationProtocolOptions) {
  async function execute<T>(operation: FileOperationExecution<T>): Promise<T> {
    const operationId = createOperationId();
    const invocation = await invokeIdempotently({
      operationId,
      invoke: operation.invoke,
    });
    if (invocation.status === 'response') {
      return admittedResponse(invocation.response, operation);
    }

    let result: Awaited<ReturnType<typeof waitForResult>> = { status: 'missing' };
    try {
      result = await waitForResult(operationId);
      if (result.status === 'completed') {
        ensureReceipt(result.receipt, operation);
        return await operation.recoverResponse(result.receipt);
      }
    } catch (error) {
      reportError('Failed to query an ambiguous file operation result', error);
    }
    if (result.status === 'failed') throw result.error;

    if (operation.recoverAmbiguous) {
      try {
        const recovered = await operation.recoverAmbiguous();
        if (recovered) return admittedResponse(recovered, operation);
      } catch (error) {
        reportError('Failed to recover an ambiguous file operation', error);
      }
    }
    throw invocation.error;
  }

  async function waitForResult(operationId: string): Promise<
    | { status: 'completed'; receipt: FileOperationReceipt }
    | { status: 'failed'; error: { code: string; message: string } }
    | { status: 'missing' }
  > {
    const discoveryDeadline = clock.now() + RESULT_DISCOVERY_DEADLINE_MS;
    let interval = INITIAL_POLL_INTERVAL_MS;
    while (true) {
      const lookup = await getFileOperationResult(operationId);
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
      if (lookup.status === 'missing' && clock.now() >= discoveryDeadline) {
        return { status: 'missing' };
      }
      await clock.sleep(interval);
      interval = Math.min(interval * 2, MAX_POLL_INTERVAL_MS);
    }
  }

  return { execute };
}

function admittedResponse<T>(response: T, operation: FileOperationExecution<T>): T {
  ensureReceipt(operation.receiptForResponse(response), operation);
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
