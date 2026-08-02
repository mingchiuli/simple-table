import {
  OperationCancelledError,
  neverCancelled,
  type OperationCancellationSignal,
} from '@/application/operationCancellation';

export class OperationOutcomeUnknownError extends Error {
  readonly operationKind: string;
  readonly operationId: string;

  constructor(operationKind: string, operationId: string) {
    super(
      `${operationKind} operation ${operationId} is still pending; its outcome could not be confirmed`,
    );
    this.name = 'OperationOutcomeUnknownError';
    this.operationKind = operationKind;
    this.operationId = operationId;
  }
}

export function isOperationOutcomeUnknown(
  error: unknown,
): error is OperationOutcomeUnknownError {
  return error instanceof OperationOutcomeUnknownError;
}

export function runBeforeDeadline<T>(
  startOperation: () => Promise<T>,
  timeoutMs: number,
  timeoutError: () => Error,
  cancellation: OperationCancellationSignal = neverCancelled,
): Promise<T> {
  if (cancellation.isCancelled()) return Promise.reject(new OperationCancelledError());

  return new Promise<T>((resolve, reject) => {
    let settled = false;
    let timeout: ReturnType<typeof setTimeout> | undefined;
    const settle = (action: () => void) => {
      if (settled) return;
      settled = true;
      if (timeout !== undefined) clearTimeout(timeout);
      unregister();
      action();
    };
    let unregister: () => void = () => undefined;
    unregister = cancellation.onCancel(() => {
      settle(() => reject(new OperationCancelledError()));
    });
    if (settled) return;

    let operation: Promise<T>;
    try {
      operation = startOperation();
    } catch (error) {
      settle(() => reject(error));
      return;
    }
    operation.then(
      (value) => settle(() => resolve(value)),
      (error) => settle(() => reject(error)),
    );
    timeout = setTimeout(() => {
      settle(() => reject(timeoutError()));
    }, timeoutMs);
  });
}
