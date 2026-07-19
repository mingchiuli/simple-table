export type ApplicationExitGuard = () => Promise<boolean>;
export type ApplicationExitIntent = 'close' | 'relaunch';

export type ApplicationExitResult =
  | { status: 'cancelled' }
  | { status: 'executed'; intent: ApplicationExitIntent };

export type ApplicationExitExecutor = {
  execute(intent: ApplicationExitIntent): Promise<void>;
};

type ActiveExitRequest = {
  intent: ApplicationExitIntent;
  actionStarted: boolean;
  promise: Promise<ApplicationExitResult>;
};

export type ApplicationExitCoordinator = ReturnType<typeof createApplicationExitCoordinator>;

export function createApplicationExitCoordinator(executor: ApplicationExitExecutor) {
  const exitGuards = new Set<ApplicationExitGuard>();
  let activeRequest: ActiveExitRequest | null = null;

  function registerGuard(guard: ApplicationExitGuard): () => void {
    exitGuards.add(guard);
    return () => exitGuards.delete(guard);
  }

  function requestExit(intent: ApplicationExitIntent): Promise<ApplicationExitResult> {
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
    for (const guard of guards) {
      if (!(await guard())) return { status: 'cancelled' };
    }

    request.actionStarted = true;
    const intent = request.intent;
    await executor.execute(intent);
    return { status: 'executed', intent };
  }

  return { registerGuard, requestExit };
}

function preferredIntent(
  current: ApplicationExitIntent,
  requested: ApplicationExitIntent,
): ApplicationExitIntent {
  return current === 'relaunch' || requested === 'relaunch' ? 'relaunch' : 'close';
}
