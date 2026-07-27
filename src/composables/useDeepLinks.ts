import { isNavigationFailure, type Router } from 'vue-router';
import { onMounted, onUnmounted } from 'vue';

import {
  createDocumentLaunchCoordinator,
} from '@/application/documentLaunchCoordinator';
import { tauriDocumentLaunchPort } from '@/platform/documentLaunchPort';

export function useDeepLinks(router: Pick<Router, 'push'>) {
  const lifecycle = createDocumentLaunchCoordinator({
    launchTargets: tauriDocumentLaunchPort,
    openTarget: async (filePath, claimId) => {
      const failure = await router.push({
        name: 'table',
        query: { file: filePath, openTargetClaim: claimId },
      });
      if (isNavigationFailure(failure)) throw failure;
    },
    reportError: (message, error) => console.error(message, error),
  });

  onMounted(lifecycle.start);
  onUnmounted(lifecycle.stop);
}
