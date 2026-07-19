import { getCurrentWindow } from '@tauri-apps/api/window';
import { inject, onMounted, onScopeDispose, type InjectionKey } from 'vue';

import type {
  ApplicationExitCoordinator,
  ApplicationExitGuard,
} from '@/application/applicationExitCoordinator';

export const applicationExitCoordinatorKey: InjectionKey<ApplicationExitCoordinator> = Symbol(
  'applicationExitCoordinator',
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
  let disposed = false;
  let unlisten: (() => void) | null = null;

  onMounted(async () => {
    if (typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) {
      return;
    }

    const appWindow = getCurrentWindow();
    try {
      const registeredUnlisten = await appWindow.onCloseRequested(async (event) => {
        event.preventDefault();
        try {
          await coordinator.requestExit('close');
        } catch (error) {
          console.error('Failed to close the application:', error);
        }
      });
      if (disposed) {
        registeredUnlisten();
      } else {
        unlisten = registeredUnlisten;
      }
    } catch (error) {
      console.error('Failed to register the application close guard:', error);
    }
  });

  onScopeDispose(() => {
    disposed = true;
    unlisten?.();
    unlisten = null;
  });
}
