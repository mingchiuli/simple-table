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

export function createOperationCancellationSource() {
  let cancelled = false;
  const handlers = new Set<() => void>();
  const signal: OperationCancellationSignal = {
    isCancelled: () => cancelled,
    onCancel(handler) {
      if (cancelled) {
        handler();
        return () => undefined;
      }
      handlers.add(handler);
      return () => handlers.delete(handler);
    },
  };

  function cancel() {
    if (cancelled) return;
    cancelled = true;
    for (const handler of handlers) handler();
    handlers.clear();
  }

  return { signal, cancel };
}

export function raceWithOperationCancellation<T>(
  operation: Promise<T>,
  cancellation: OperationCancellationSignal,
): Promise<T> {
  if (cancellation.isCancelled()) return Promise.reject(new OperationCancelledError());
  return new Promise<T>((resolve, reject) => {
    const unregister = cancellation.onCancel(() => reject(new OperationCancelledError()));
    operation.then(resolve, reject).finally(unregister);
  });
}

export function isOperationCancelled(error: unknown): error is OperationCancelledError {
  return error instanceof OperationCancelledError;
}
