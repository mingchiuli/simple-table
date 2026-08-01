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

const LISTENER_REGISTRATION_REPORT_INTERVAL = 3;
const LISTENER_REGISTRATION_RETRY_MS = 250;
const LISTENER_REGISTRATION_TIMEOUT_MS = 5_000;
const DEFAULT_EXIT_GUARD_TIMEOUT_MS = 30_000;

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

  return { registerGuard, requestExit, dispose };
}

export function createWindowCloseRequestLifecycle(
  applicationWindow: Pick<ApplicationWindowPort, 'subscribeCloseRequested'>,
  coordinator: Pick<ApplicationExitCoordinator, 'requestExit'>,
  reportError: (message: string, error: unknown) => void = (message, error) => {
    console.error(message, error);
  },
  waitBeforeRetry: () => Promise<void> = () => new Promise(
    (resolve) => setTimeout(resolve, LISTENER_REGISTRATION_RETRY_MS),
  ),
  registrationTimeoutMs = LISTENER_REGISTRATION_TIMEOUT_MS,
) {
  let disposed = false;
  let unlisten: (() => void) | null = null;
  let registration: Promise<void> | null = null;

  function start(): Promise<void> {
    if (disposed) return Promise.resolve();
    registration ??= registerListener();
    return registration;
  }

  async function registerListener(): Promise<void> {
    let lastError: unknown;
    let failures = 0;
    while (!disposed) {
      try {
        const registeredUnlisten = await subscribeBeforeDeadline(
          () => applicationWindow.subscribeCloseRequested(async () => {
            try {
              await coordinator.requestExit('close');
            } catch (error) {
              reportError('Failed to close the application:', error);
            }
          }),
          registrationTimeoutMs,
        );
        if (disposed) {
          registeredUnlisten();
        } else {
          unlisten = registeredUnlisten;
        }
        return;
      } catch (error) {
        lastError = error;
        failures += 1;
        if (failures % LISTENER_REGISTRATION_REPORT_INTERVAL === 0) {
          reportError('Failed to register the application close request listener:', lastError);
        }
      }
      if (!disposed) await waitBeforeRetry();
    }
  }

  function dispose() {
    disposed = true;
    unlisten?.();
    unlisten = null;
  }

  return { start, dispose };
}

async function subscribeBeforeDeadline(
  subscribe: () => Promise<() => void>,
  timeoutMs: number,
): Promise<() => void> {
  let expired = false;
  let timeout: ReturnType<typeof setTimeout> | undefined;
  const subscription = subscribe();
  void subscription.then(
    (unlisten) => {
      if (expired) unlisten();
    },
    () => undefined,
  );
  const deadline = new Promise<never>((_, reject) => {
    timeout = setTimeout(() => {
      expired = true;
      reject(new Error(`Close-listener registration timed out after ${timeoutMs} ms`));
    }, timeoutMs);
  });

  try {
    return await Promise.race([subscription, deadline]);
  } finally {
    if (timeout !== undefined) clearTimeout(timeout);
  }
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
      reject(new Error(`Application exit preparation timed out after ${timeoutMs} ms`));
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
