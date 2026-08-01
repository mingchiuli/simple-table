import type { OperationCancellationSignal } from '@/application/operationCancellation';

export type DocumentPreparationCoordinator = ReturnType<
  typeof createDocumentPreparationCoordinator
>;

type DocumentPreparationCoordinatorOptions = {
  cleanupAttempts?: number;
  reportCleanupFailure?: (error: unknown) => void;
};

type PendingPreparationCleanup = {
  discard: (preparationId: string) => Promise<void>;
  reportFailure?: (error: unknown) => void;
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
  const activePreparationIdCleanups = new Set<Promise<boolean>>();
  const cleanupFailures: unknown[] = [];
  const pendingCleanupIds = new Map<string, PendingPreparationCleanup>();
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

  function cleanupPreparationId(
    preparationId: string,
    discard: (preparationId: string) => Promise<void>,
    reportFailure?: (error: unknown) => void,
  ): Promise<boolean> {
    return trackPreparationIdCleanup(
      cleanupPreparationIdNow(preparationId, discard, reportFailure),
    );
  }

  async function cleanupPreparationIdNow(
    preparationId: string,
    discard: (preparationId: string) => Promise<void>,
    reportFailure?: (error: unknown) => void,
  ): Promise<boolean> {
    try {
      await discardWithRetry(preparationId, discard);
      pendingCleanupIds.delete(preparationId);
      return true;
    } catch (error) {
      pendingCleanupIds.set(preparationId, { discard, reportFailure });
      safeReportFailure(reportFailure, error);
      return false;
    }
  }

  function drainPreparationCleanupIds(
    discard: (preparationId: string) => Promise<void>,
    reportFailure?: (error: unknown) => void,
  ): Promise<boolean> {
    return trackPreparationIdCleanup((async () => {
      let cleaned = true;
      for (const preparationId of [...pendingCleanupIds.keys()]) {
        if (!(await cleanupPreparationIdNow(preparationId, discard, reportFailure))) {
          cleaned = false;
        }
      }
      return cleaned;
    })());
  }

  function trackPreparationIdCleanup(cleanup: Promise<boolean>): Promise<boolean> {
    activePreparationIdCleanups.add(cleanup);
    void cleanup.finally(() => activePreparationIdCleanups.delete(cleanup));
    return cleanup;
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
    while (activePreparationIdCleanups.size > 0) {
      await Promise.allSettled(activePreparationIdCleanups);
    }

    const failures = cleanupFailures.splice(0);
    for (const [preparationId, cleanup] of [...pendingCleanupIds]) {
      try {
        await discardWithRetry(preparationId, cleanup.discard);
        pendingCleanupIds.delete(preparationId);
      } catch (error) {
        failures.push(error);
        safeReportFailure(cleanup.reportFailure, error);
      }
    }
    if (failures.length > 0) {
      throw new DocumentPreparationCleanupError(failures);
    }
  }

  function safeReportFailure(
    reportFailure: ((error: unknown) => void) | undefined,
    error: unknown,
  ) {
    try {
      reportFailure?.(error);
    } catch {
      // Cleanup reporting must not interrupt preparation ownership.
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
