import type { OperationCancellationSignal } from '@/application/operationCancellation';

export type DocumentRoutePreparationScheduler = ReturnType<
  typeof createDocumentRoutePreparationScheduler
>;

export function createDocumentRoutePreparationScheduler() {
  let tail: Promise<void> = Promise.resolve();

  async function run<T>(
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

    const prepared = tail.then(async () => {
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
    tail = prepared.then(
      () => undefined,
      () => undefined,
    );

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

  return { run };
}
