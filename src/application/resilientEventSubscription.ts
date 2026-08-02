export type EventSubscription = {
  start(): Promise<void>;
  dispose(): void;
};

type ResilientEventSubscriptionOptions = {
  subscribe(handler: () => void | Promise<void>): Promise<() => void>;
  handler: () => void | Promise<void>;
  reportError(message: string, error: unknown): void;
  registrationErrorMessage: string;
  cleanupErrorMessage: string;
  onSubscribed?: () => void;
  waitBeforeRetry?: () => Promise<void>;
  registrationTimeoutMs?: number;
  reportInterval?: number;
};

const DEFAULT_RETRY_MS = 250;
const DEFAULT_REGISTRATION_TIMEOUT_MS = 5_000;
const DEFAULT_REPORT_INTERVAL = 3;

export function createResilientEventSubscription({
  subscribe,
  handler,
  reportError,
  registrationErrorMessage,
  cleanupErrorMessage,
  onSubscribed,
  waitBeforeRetry = () => new Promise((resolve) => setTimeout(resolve, DEFAULT_RETRY_MS)),
  registrationTimeoutMs = DEFAULT_REGISTRATION_TIMEOUT_MS,
  reportInterval = DEFAULT_REPORT_INTERVAL,
}: ResilientEventSubscriptionOptions): EventSubscription {
  let disposed = false;
  let unlisten: (() => void) | null = null;
  let registration: Promise<void> | null = null;
  let stopRetry!: () => void;
  const stopped = new Promise<void>((resolve) => {
    stopRetry = resolve;
  });

  function start(): Promise<void> {
    if (disposed) return Promise.resolve();
    registration ??= register();
    return registration;
  }

  async function register() {
    let failures = 0;
    while (!disposed) {
      try {
        const registered = await subscribeBeforeDeadline(
          () => subscribe(handler),
          registrationTimeoutMs,
          stopped,
          safeUnlisten,
        );
        if (!registered) return;
        if (disposed) {
          safeUnlisten(registered);
          return;
        }
        unlisten = registered;
        try {
          onSubscribed?.();
        } catch (error) {
          safeReport('Failed to finish event-listener registration:', error);
        }
        return;
      } catch (error) {
        failures += 1;
        if (failures % reportInterval === 0) {
          safeReport(registrationErrorMessage, error);
        }
      }
      if (!disposed) await Promise.race([waitBeforeRetry(), stopped]);
    }
  }

  function dispose() {
    if (disposed) return;
    disposed = true;
    stopRetry();
    safeUnlisten(unlisten);
    unlisten = null;
  }

  function safeUnlisten(registered: (() => void) | null) {
    try {
      registered?.();
    } catch (error) {
      safeReport(cleanupErrorMessage, error);
    }
  }

  function safeReport(message: string, error: unknown) {
    try {
      reportError(message, error);
    } catch {
      // Diagnostics must not break subscription ownership or cleanup.
    }
  }

  return { start, dispose };
}

async function subscribeBeforeDeadline(
  subscribe: () => Promise<() => void>,
  timeoutMs: number,
  stopped: Promise<void>,
  abandon: (unlisten: () => void) => void,
): Promise<(() => void) | null> {
  let abandoned = false;
  let timeout: ReturnType<typeof setTimeout> | undefined;
  const subscription = subscribe();
  void subscription.then(
    (unlisten) => {
      if (abandoned) abandon(unlisten);
    },
    () => undefined,
  );
  const deadline = new Promise<'timeout'>((resolve) => {
    timeout = setTimeout(() => resolve('timeout'), timeoutMs);
  });
  const stop = stopped.then(() => 'stopped' as const);

  try {
    const result = await Promise.race([
      subscription.then((unlisten) => ({ status: 'subscribed' as const, unlisten })),
      deadline.then((status) => ({ status })),
      stop.then((status) => ({ status })),
    ]);
    if (result.status === 'subscribed') return result.unlisten;
    abandoned = true;
    if (result.status === 'timeout') {
      throw new Error(`Event-listener registration timed out after ${timeoutMs} ms`);
    }
    return null;
  } finally {
    if (timeout !== undefined) clearTimeout(timeout);
  }
}
