import {
  createUpdateCoordinator,
  type UpdateCoordinator,
} from '@/application/updateCoordinator';
import { tauriUpdatePort } from '@/platform/updatePort';
import { useUpdateSessionStore } from '@/stores/updateSession';

const coordinators = new WeakMap<object, UpdateCoordinator>();

export function useUpdateCoordinator(): UpdateCoordinator {
  const store = useUpdateSessionStore();
  let coordinator = coordinators.get(store);
  if (!coordinator) {
    coordinator = createUpdateCoordinator(store, tauriUpdatePort);
    coordinators.set(store, coordinator);
  }
  return coordinator;
}
