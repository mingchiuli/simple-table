import type { OperationCancellationSignal } from '@/application/operationCancellation';

export type DocumentPreparationCoordinator = ReturnType<
  typeof createDocumentPreparationCoordinator
>;

export function createDocumentPreparationCoordinator() {
  let tail: Promise<void> = Promise.resolve();

  function run<T>(prepare: () => Promise<T>): Promise<T> {
    return enqueue(prepare);
  }

  async function runCancellable<T>(
    prepare: () => Promise<T>,
    cancellation: OperationCancellationSignal,
    discard: (result: T) => Promise<void>,
  ): Promise<T | undefined> {
    let cancelled = cancellation.isCancelled();
    let notifyCancellation!: () => void;
    const cancellationResult = new Promise<{ status: 'cancelled' }>((resolve) => {
      notifyCancellation = () => resolve({ status: 'cancelled' });
    });
    if (cancelled) notifyCancellation();
    const unregister = cancellation.onCancel(() => {
      cancelled = true;
      notifyCancellation();
    });

    const prepared = enqueue(async () => {
      if (cancelled) return undefined;
      try {
        const result = await prepare();
        if (!cancelled) return result;
        await discard(result);
        return undefined;
      } catch (error) {
        if (cancelled) return undefined;
        throw error;
      }
    });

    try {
      const outcome = await Promise.race([
        prepared.then((value) => ({ status: 'prepared' as const, value })),
        cancellationResult,
      ]);
      return outcome.status === 'prepared' ? outcome.value : undefined;
    } finally {
      unregister();
    }
  }

  function enqueue<T>(task: () => Promise<T>): Promise<T> {
    const result = tail.then(task);
    tail = result.then(
      () => undefined,
      () => undefined,
    );
    return result;
  }

  return { run, runCancellable };
}
