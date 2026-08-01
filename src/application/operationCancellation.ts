export type OperationCancellationSignal = {
  isCancelled(): boolean;
  onCancel(handler: () => void): () => void;
};

export const neverCancelled: OperationCancellationSignal = {
  isCancelled: () => false,
  onCancel: () => () => undefined,
};

export class OperationCancelledError extends Error {
  constructor() {
    super('Operation was cancelled because its workspace is being disposed');
    this.name = 'OperationCancelledError';
  }
}

export function throwIfOperationCancellationFailed(
  failures: readonly unknown[],
  message: string,
): void {
  if (failures.length > 0) throw new AggregateError(failures, message);
}

export function createOperationCancellationSource() {
  let cancelled = false;
  const handlers = new Set<() => void>();
  const notificationFailures: unknown[] = [];
  const signal: OperationCancellationSignal = {
    isCancelled: () => cancelled,
    onCancel(handler) {
      if (cancelled) {
        try {
          handler();
        } catch (error) {
          notificationFailures.push(error);
        }
        return () => undefined;
      }
      handlers.add(handler);
      return () => handlers.delete(handler);
    },
  };

  function cancel(): readonly unknown[] {
    if (cancelled) return notificationFailures;
    cancelled = true;
    const pendingHandlers = [...handlers];
    handlers.clear();
    for (const handler of pendingHandlers) {
      try {
        handler();
      } catch (error) {
        notificationFailures.push(error);
      }
    }
    return notificationFailures;
  }

  return { signal, cancel };
}

export function raceWithOperationCancellation<T>(
  startOperation: () => Promise<T>,
  cancellation: OperationCancellationSignal,
): Promise<T> {
  if (cancellation.isCancelled()) return Promise.reject(new OperationCancelledError());
  return new Promise<T>((resolve, reject) => {
    let settled = false;
    const settle = (action: () => void) => {
      if (settled) return;
      settled = true;
      action();
    };
    const unregister = cancellation.onCancel(() => {
      settle(() => reject(new OperationCancelledError()));
    });
    if (settled) return;

    let operation: Promise<T>;
    try {
      operation = startOperation();
    } catch (error) {
      settle(() => {
        unregister();
        reject(error);
      });
      return;
    }
    operation.then(
      (value) => settle(() => {
        unregister();
        resolve(value);
      }),
      (error) => settle(() => {
        unregister();
        reject(error);
      }),
    );
  });
}

export function isOperationCancelled(error: unknown): error is OperationCancelledError {
  return error instanceof OperationCancelledError;
}
