import {
  createRouteDocumentLoadCoordinator,
  type RouteDocumentLoadCoordinator,
  type RouteDocumentLoadPorts,
} from '@/application/routeDocumentLoadCoordinator';
import { ElMessage } from 'element-plus';
import { acknowledgeOpenTarget, renewOpenTarget } from '@/platform';
import { appErrorMessage } from '@/utils/appError';
import { onScopeDispose } from 'vue';
import { useApplicationWorkspaceRuntime } from '@/composables/applicationWorkspaceRuntime';

type UseRouteFileLoaderOptions = Omit<
  RouteDocumentLoadPorts,
  'acknowledgeOpenTarget' | 'renewOpenTarget' | 'reportError'
> & {
  acknowledgeOpenTarget?: RouteDocumentLoadPorts['acknowledgeOpenTarget'];
  renewOpenTarget?: RouteDocumentLoadPorts['renewOpenTarget'];
  reportError?: (error: unknown) => void;
};

type RouteLeaveHandlerOptions = {
  routeFileLoader: Pick<RouteDocumentLoadCoordinator, 'dispose'>;
  hasActiveDocument: () => boolean;
  closeCurrentDocument: () => Promise<boolean>;
};

export function useRouteFileLoader({
  acknowledgeOpenTarget: acknowledge = acknowledgeOpenTarget,
  renewOpenTarget: renew = renewOpenTarget,
  reportError = (error) => {
    ElMessage.error(`Failed to open file: ${appErrorMessage(error)}`);
  },
  ...ports
}: UseRouteFileLoaderOptions): RouteDocumentLoadCoordinator {
  const coordinator = createRouteDocumentLoadCoordinator({
    ...ports,
    acknowledgeOpenTarget: acknowledge,
    renewOpenTarget: renew,
    reportError,
  });
  const releaseOwnership = useApplicationWorkspaceRuntime().registerRouteDocumentLoad(coordinator);
  let disposal: Promise<void> | null = null;
  const ownedCoordinator: RouteDocumentLoadCoordinator = {
    ...coordinator,
    dispose() {
      disposal ??= coordinator.dispose().finally(releaseOwnership);
      return disposal;
    },
  };
  onScopeDispose(() => {
    void ownedCoordinator.dispose().catch((error) => {
      console.error('Failed to dispose route document loading:', error);
    });
  });
  return ownedCoordinator;
}

export function createRouteLeaveHandler({
  routeFileLoader,
  hasActiveDocument,
  closeCurrentDocument,
}: RouteLeaveHandlerOptions) {
  return async () => {
    if (!hasActiveDocument()) {
      await disposeRouteFileLoader(routeFileLoader);
      return true;
    }

    const canLeave = await closeCurrentDocument();
    if (canLeave) {
      await disposeRouteFileLoader(routeFileLoader);
    }
    return canLeave;
  };
}

async function disposeRouteFileLoader(
  routeFileLoader: Pick<RouteDocumentLoadCoordinator, 'dispose'>,
) {
  try {
    await routeFileLoader.dispose();
  } catch (error) {
    console.error('Failed to dispose route document loading:', error);
  }
}
