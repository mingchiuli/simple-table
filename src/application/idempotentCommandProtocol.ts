import { isAppErrorPayload } from '@/utils/appError';
import {
  isOperationCancelled,
  neverCancelled,
  type OperationCancellationSignal,
} from '@/application/operationCancellation';
import {
  isOperationOutcomeUnknown,
  OperationOutcomeUnknownError,
  runBeforeDeadline,
} from '@/application/operationOutcome';

export type IdempotentInvokeResult<T> =
  | { status: 'response'; response: T }
  | { status: 'ambiguous'; error: unknown };

type IdempotentInvokeOptions<T> = {
  operationId: string;
  invoke: (operationId: string) => Promise<T>;
  isDefinitiveFailure?: (error: unknown) => boolean;
  responseTimeoutMs?: number;
  timeoutError?: () => Error;
  cancellation?: OperationCancellationSignal;
};

export async function invokeIdempotently<T>({
  operationId,
  invoke,
  isDefinitiveFailure = isAppErrorPayload,
  responseTimeoutMs = 30_000,
  timeoutError = () => new OperationOutcomeUnknownError('command', operationId),
  cancellation = neverCancelled,
}: IdempotentInvokeOptions<T>): Promise<IdempotentInvokeResult<T>> {
  try {
    return {
      status: 'response',
      response: await runBeforeDeadline(
        () => invoke(operationId),
        responseTimeoutMs,
        timeoutError,
        cancellation,
      ),
    };
  } catch (error) {
    if (
      isDefinitiveFailure(error)
      || isOperationCancelled(error)
      || isOperationOutcomeUnknown(error)
    ) {
      if (isOperationOutcomeUnknown(error)) return { status: 'ambiguous', error };
      throw error;
    }
  }

  try {
    return {
      status: 'response',
      response: await runBeforeDeadline(
        () => invoke(operationId),
        responseTimeoutMs,
        timeoutError,
        cancellation,
      ),
    };
  } catch (error) {
    if (isOperationCancelled(error)) throw error;
    return { status: 'ambiguous', error };
  }
}
