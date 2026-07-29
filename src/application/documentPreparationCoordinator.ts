import type { OperationCancellationSignal } from '@/application/operationCancellation';

export type DocumentPreparationCoordinator = ReturnType<
  typeof createDocumentPreparationCoordinator
>;

type DocumentPreparationCoordinatorOptions = {
  cleanupAttempts?: number;
  reportCleanupFailure?: (error: unknown) => void;
};

export class DocumentPreparationCleanupError extends Error {
  constructor(readonly failures: readonly unknown[]) {
    super(`Failed to clean up ${failures.length} cancelled document preparation task(s)`);
    this.name = 'DocumentPreparationCleanupError';
  }
}

const DEFAULT_CLEANUP_ATTEMPTS = 3;

export function createDocumentPreparationCoordinator(
  options: DocumentPreparationCoordinatorOptions = {},
) {
  let tail: Promise<void> = Promise.resolve();
  const activeCleanupObservers = new Set<Promise<void>>();
  const cleanupFailures: unknown[] = [];
  const pendingCleanupIds = new Set<string>();
  const cleanupAttempts = Math.max(1, options.cleanupAttempts ?? DEFAULT_CLEANUP_ATTEMPTS);

  function run<T>(prepare: () => Promise<T>): Promise<T> {
    return enqueue(prepare);
  }

  function cleanup<T>(result: T, discard: (result: T) => Promise<void>): Promise<void> {
    return enqueue(() => discardWithRetry(result, discard));
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
      let result: T;
      try {
        result = await prepare();
      } catch (error) {
        if (cancelled) return undefined;
        throw error;
      }
      if (!cancelled) return result;
      await discardWithRetry(result, discard);
      return undefined;
    });

    try {
      const outcome = await Promise.race([
        prepared.then((value) => ({ status: 'prepared' as const, value })),
        cancellationResult,
      ]);
      if (outcome.status === 'prepared') return outcome.value;
      observeCancelledCleanup(prepared);
      return undefined;
    } finally {
      unregister();
    }
  }

  async function discardWithRetry<T>(
    result: T,
    discard: (result: T) => Promise<void>,
  ): Promise<void> {
    let lastFailure: unknown;
    for (let attempt = 0; attempt < cleanupAttempts; attempt += 1) {
      try {
        await discard(result);
        return;
      } catch (error) {
        lastFailure = error;
        if (attempt + 1 < cleanupAttempts) await Promise.resolve();
      }
    }
    throw lastFailure;
  }

  async function cleanupPreparationId(
    preparationId: string,
    discard: (preparationId: string) => Promise<void>,
    reportFailure?: (error: unknown) => void,
  ): Promise<boolean> {
    try {
      await discardWithRetry(preparationId, discard);
      pendingCleanupIds.delete(preparationId);
      return true;
    } catch (error) {
      pendingCleanupIds.add(preparationId);
      reportFailure?.(error);
      return false;
    }
  }

  async function drainPreparationCleanupIds(
    discard: (preparationId: string) => Promise<void>,
    reportFailure?: (error: unknown) => void,
  ): Promise<boolean> {
    for (const preparationId of [...pendingCleanupIds]) {
      if (!(await cleanupPreparationId(preparationId, discard, reportFailure))) return false;
    }
    return true;
  }

  function observeCancelledCleanup(cleanup: Promise<unknown>) {
    const observer = cleanup.then(
      () => undefined,
      (error) => {
        cleanupFailures.push(error);
        try {
          options.reportCleanupFailure?.(error);
        } catch {
          // Failure reporting must not break preparation queue ownership.
        }
      },
    );
    activeCleanupObservers.add(observer);
    void observer.finally(() => activeCleanupObservers.delete(observer));
  }

  function enqueue<T>(task: () => Promise<T>): Promise<T> {
    const result = tail.then(task);
    tail = result.then(
      () => undefined,
      () => undefined,
    );
    return result;
  }

  async function waitForIdle(): Promise<void> {
    await tail;
    while (activeCleanupObservers.size > 0) {
      await Promise.allSettled(activeCleanupObservers);
    }
    if (cleanupFailures.length > 0) {
      throw new DocumentPreparationCleanupError(cleanupFailures.splice(0));
    }
  }

  return {
    run,
    runCancellable,
    cleanup,
    cleanupPreparationId,
    drainPreparationCleanupIds,
    waitForIdle,
  };
}
