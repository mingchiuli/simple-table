import { inject, onScopeDispose, type InjectionKey } from 'vue';

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
