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
    const preparations: ApplicationExitPreparation[] = [];
    for (const guard of guards) {
      try {
        const preparation = await guard();
        if (!preparation) {
          rollbackPreparations(preparations);
          return { status: 'cancelled' };
        }
        preparations.push(preparation);
      } catch (error) {
        rollbackPreparations(preparations);
        throw error;
      }
    }

    request.actionStarted = true;
    const intent = request.intent;
    try {
      await executor.execute(intent);
    } catch (error) {
      rollbackPreparations(preparations);
      throw error;
    }
    commitPreparations(preparations);
    return { status: 'executed', intent };
  }

  return { registerGuard, requestExit };
}

function commitPreparations(preparations: ApplicationExitPreparation[]) {
  for (const preparation of preparations) preparation.commit();
}

function rollbackPreparations(preparations: ApplicationExitPreparation[]) {
  for (let index = preparations.length - 1; index >= 0; index -= 1) {
    preparations[index]?.rollback();
  }
}

function preferredIntent(
  current: ApplicationExitIntent,
  requested: ApplicationExitIntent,
): ApplicationExitIntent {
  return current === 'relaunch' || requested === 'relaunch' ? 'relaunch' : 'close';
}
