import {
  createUpdateCoordinator,
  type UpdateCoordinator,
} from '@/application/updateCoordinator';
import { tauriUpdatePort } from '@/platform/updatePort';
import { useUpdateSessionStore } from '@/stores/updateSession';
import { useApplicationExitCoordinator } from '@/composables/useApplicationExit';

const coordinators = new WeakMap<object, UpdateCoordinator>();

export function useUpdateCoordinator(): UpdateCoordinator {
  const store = useUpdateSessionStore();
  const applicationExit = useApplicationExitCoordinator();
  let coordinator = coordinators.get(store);
  if (!coordinator) {
    coordinator = createUpdateCoordinator(store, tauriUpdatePort, {
      requestRelaunch: async () => {
        const result = await applicationExit.requestExit('relaunch');
        return result.status === 'executed' && result.intent === 'relaunch';
      },
    });
    coordinators.set(store, coordinator);
  }
  return coordinator;
}
