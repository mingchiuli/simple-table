import { createResilientEventSubscription } from '@/application/resilientEventSubscription';

export type ApplicationExitPreparation = {
  commit(): void;
  rollback(): void;
};

export type ApplicationExitGuard = () => Promise<ApplicationExitPreparation | null>;
export type ApplicationExitIntent = 'close' | 'relaunch';

export type ApplicationExitResult =
  | { status: 'cancelled' }
  | { status: 'executed'; intent: ApplicationExitIntent };

export type ApplicationExitExecutor = {
  execute(intent: ApplicationExitIntent): Promise<void>;
};

export type ApplicationWindowPort = ApplicationExitExecutor & {
  subscribeCloseRequested(handler: () => void | Promise<void>): Promise<() => void>;
};

const DEFAULT_EXIT_GUARD_TIMEOUT_MS = 30_000;

export class ApplicationExitPreparationTimeoutError extends Error {
  constructor(timeoutMs: number) {
    super(`Application exit preparation timed out after ${timeoutMs} ms`);
    this.name = 'ApplicationExitPreparationTimeoutError';
  }
}

type ApplicationExitCoordinatorOptions = {
  guardTimeoutMs?: number;
};

type ActiveExitRequest = {
  intent: ApplicationExitIntent;
  actionStarted: boolean;
  promise: Promise<ApplicationExitResult>;
};

export type ApplicationExitCoordinator = ReturnType<typeof createApplicationExitCoordinator>;

export function createApplicationExitCoordinator(
  executor: ApplicationExitExecutor,
  { guardTimeoutMs = DEFAULT_EXIT_GUARD_TIMEOUT_MS }: ApplicationExitCoordinatorOptions = {},
) {
  const exitGuards = new Set<ApplicationExitGuard>();
  let activeRequest: ActiveExitRequest | null = null;
  let disposed = false;
  let disposal: Promise<void> | null = null;

  function registerGuard(guard: ApplicationExitGuard): () => void {
    if (disposed) return () => undefined;
    exitGuards.add(guard);
    return () => exitGuards.delete(guard);
  }

  function requestExit(intent: ApplicationExitIntent): Promise<ApplicationExitResult> {
    if (disposed) return Promise.resolve({ status: 'cancelled' });
    if (activeRequest) {
      if (!activeRequest.actionStarted) {
        activeRequest.intent = preferredIntent(activeRequest.intent, intent);
      }
      return activeRequest.promise;
    }

    const request: ActiveExitRequest = {
      intent,
      actionStarted: false,
      promise: Promise.resolve({ status: 'cancelled' }),
    };
    request.promise = runExitRequest(request).finally(() => {
      if (activeRequest === request) activeRequest = null;
    });
    activeRequest = request;
    return request.promise;
  }

  async function runExitRequest(request: ActiveExitRequest): Promise<ApplicationExitResult> {
    const guards = Array.from(exitGuards).reverse();
    const preparations: ApplicationExitPreparation[] = [];
    for (const guard of guards) {
      if (disposed) {
        throwIfSettlementFailed(
          rollbackPreparations(preparations),
          'Application exit was cancelled but one or more preparations failed to roll back',
        );
        return { status: 'cancelled' };
      }
      let preparation: ApplicationExitPreparation | null;
      try {
        preparation = await prepareGuardBeforeDeadline(guard, guardTimeoutMs);
      } catch (error) {
        throw combineWithSettlementFailures(
          error,
          rollbackPreparations(preparations),
          'Application exit preparation failed and rollback was incomplete',
        );
      }
      if (!preparation) {
        throwIfSettlementFailed(
          rollbackPreparations(preparations),
          'Application exit was cancelled but one or more preparations failed to roll back',
        );
        return { status: 'cancelled' };
      }
      preparations.push(preparation);
      if (disposed) {
        throwIfSettlementFailed(
          rollbackPreparations(preparations),
          'Application exit was cancelled but one or more preparations failed to roll back',
        );
        return { status: 'cancelled' };
      }
    }

    request.actionStarted = true;
    const intent = request.intent;
    try {
      await executor.execute(intent);
    } catch (error) {
      throw combineWithSettlementFailures(
        error,
        rollbackPreparations(preparations),
        'Application exit execution failed and rollback was incomplete',
      );
    }
    throwIfSettlementFailed(
      commitPreparations(preparations),
      'Application exit executed but one or more preparations failed to commit',
    );
    return { status: 'executed', intent };
  }

  function dispose(): Promise<void> {
    if (disposal) return disposal;
    disposed = true;
    exitGuards.clear();
    const active = activeRequest?.promise;
    disposal = active
      ? Promise.allSettled([active]).then(() => undefined)
      : Promise.resolve();
    return disposal;
  }

  async function forceExit(intent: ApplicationExitIntent): Promise<ApplicationExitResult> {
    if (disposed) return { status: 'cancelled' };
    const pending = activeRequest?.promise;
    if (pending) await Promise.allSettled([pending]);
    if (disposed) return { status: 'cancelled' };
    await executor.execute(intent);
    return { status: 'executed', intent };
  }

  return { registerGuard, requestExit, forceExit, dispose };
}

export function createWindowCloseRequestLifecycle(
  applicationWindow: Pick<ApplicationWindowPort, 'subscribeCloseRequested'>,
  coordinator: Pick<ApplicationExitCoordinator, 'requestExit'>
    & Partial<Pick<ApplicationExitCoordinator, 'forceExit'>>,
  reportError: (message: string, error: unknown) => void = (message, error) => {
    console.error(message, error);
  },
  waitBeforeRetry: () => Promise<void> = () => new Promise(
    (resolve) => setTimeout(resolve, 250),
  ),
  registrationTimeoutMs = 5_000,
  confirmForceExit: () => boolean = defaultConfirmForceExit,
) {
  const subscription = createResilientEventSubscription({
    subscribe: (handler) => applicationWindow.subscribeCloseRequested(handler),
    handler: async () => {
      try {
        await coordinator.requestExit('close');
      } catch (error) {
        if (
          error instanceof ApplicationExitPreparationTimeoutError
          && coordinator.forceExit
          && confirmForceExit()
        ) {
          try {
            await coordinator.forceExit('close');
            return;
          } catch (forceError) {
            reportError('Failed to force close the application:', forceError);
          }
        }
        reportError('Failed to close the application:', error);
      }
    },
    reportError,
    registrationErrorMessage: 'Failed to register the application close request listener:',
    cleanupErrorMessage: 'Failed to clean up the application close request listener:',
    waitBeforeRetry,
    registrationTimeoutMs,
  });

  return subscription;
}

async function prepareGuardBeforeDeadline(
  guard: ApplicationExitGuard,
  timeoutMs: number,
): Promise<ApplicationExitPreparation | null> {
  let expired = false;
  let timeout: ReturnType<typeof setTimeout> | undefined;
  const preparation = Promise.resolve().then(guard);
  const deadline = new Promise<never>((_, reject) => {
    timeout = setTimeout(() => {
      expired = true;
      reject(new ApplicationExitPreparationTimeoutError(timeoutMs));
    }, timeoutMs);
  });

  void preparation.then(
    (latePreparation) => {
      if (!expired || !latePreparation) return;
      try {
        latePreparation.rollback();
      } catch (error) {
        console.error('Failed to roll back a late application exit preparation:', error);
      }
    },
    () => undefined,
  );

  try {
    return await Promise.race([preparation, deadline]);
  } finally {
    if (timeout !== undefined) clearTimeout(timeout);
  }
}

function defaultConfirmForceExit(): boolean {
  return typeof window !== 'undefined'
    && window.confirm(
      'An operation is still pending. Force closing may leave the latest changes unsaved. Close anyway?',
    );
}

function commitPreparations(preparations: ApplicationExitPreparation[]): unknown[] {
  return settlePreparations(preparations, (preparation) => preparation.commit());
}

function rollbackPreparations(preparations: ApplicationExitPreparation[]): unknown[] {
  return settlePreparations(
    [...preparations].reverse(),
    (preparation) => preparation.rollback(),
  );
}

function settlePreparations(
  preparations: readonly ApplicationExitPreparation[],
  settle: (preparation: ApplicationExitPreparation) => void,
): unknown[] {
  const failures: unknown[] = [];
  for (const preparation of preparations) {
    try {
      settle(preparation);
    } catch (error) {
      failures.push(error);
    }
  }
  return failures;
}

function throwIfSettlementFailed(failures: unknown[], message: string): void {
  if (failures.length > 0) throw new AggregateError(failures, message);
}

function combineWithSettlementFailures(
  primary: unknown,
  settlementFailures: unknown[],
  message: string,
): unknown {
  return settlementFailures.length > 0
    ? new AggregateError([primary, ...settlementFailures], message)
    : primary;
}

function preferredIntent(
  current: ApplicationExitIntent,
  requested: ApplicationExitIntent,
): ApplicationExitIntent {
  return current === 'relaunch' || requested === 'relaunch' ? 'relaunch' : 'close';
}
