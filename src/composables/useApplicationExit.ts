import { inject, onMounted, onScopeDispose, type InjectionKey } from 'vue';

import type {
  ApplicationExitCoordinator,
  ApplicationExitGuard,
  ApplicationWindowPort,
} from '@/application/applicationExitCoordinator';

export const applicationExitCoordinatorKey: InjectionKey<ApplicationExitCoordinator> = Symbol(
  'applicationExitCoordinator',
);
export const applicationWindowPortKey: InjectionKey<ApplicationWindowPort> = Symbol(
  'applicationWindowPort',
);

export function useApplicationExitCoordinator(): ApplicationExitCoordinator {
  const coordinator = inject(applicationExitCoordinatorKey);
  if (!coordinator) {
    throw new Error('Application exit coordinator is not provided');
  }
  return coordinator;
}

export function useApplicationExitGuard(guard: ApplicationExitGuard) {
  const unregister = useApplicationExitCoordinator().registerGuard(guard);
  onScopeDispose(unregister);
}

export function useWindowCloseGuard() {
  const coordinator = useApplicationExitCoordinator();
  const applicationWindow = inject(applicationWindowPortKey);
  if (!applicationWindow) {
    throw new Error('Application window port is not provided');
  }
  const lifecycle = createWindowCloseGuardLifecycle(applicationWindow, coordinator);

  onMounted(() => {
    void lifecycle.start();
  });
  onScopeDispose(lifecycle.dispose);
}

export function createWindowCloseGuardLifecycle(
  applicationWindow: Pick<ApplicationWindowPort, 'subscribeCloseRequested'>,
  coordinator: Pick<ApplicationExitCoordinator, 'requestExit'>,
  reportError: (message: string, error: unknown) => void = (message, error) => {
    console.error(message, error);
  },
) {
  let disposed = false;
  let unlisten: (() => void) | null = null;
  let registration: Promise<void> | null = null;

  function start(): Promise<void> {
    if (disposed) return Promise.resolve();
    registration ??= applicationWindow.subscribeCloseRequested(async () => {
      try {
        await coordinator.requestExit('close');
      } catch (error) {
        reportError('Failed to close the application:', error);
      }
    }).then((registeredUnlisten) => {
      if (disposed) {
        registeredUnlisten();
      } else {
        unlisten = registeredUnlisten;
      }
    }).catch((error) => {
      reportError('Failed to register the application close guard:', error);
    });
    return registration;
  }

  function dispose() {
    disposed = true;
    unlisten?.();
    unlisten = null;
  }

  return { start, dispose };
}
