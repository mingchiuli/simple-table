import {
  createRouteDocumentLoadCoordinator,
  type RouteDocumentLoadCoordinator,
  type RouteDocumentLoadPorts,
} from '@/application/routeDocumentLoadCoordinator';
import { acknowledgeOpenTarget, releaseOpenTarget } from '@/platform';
import { onScopeDispose } from 'vue';

type UseRouteFileLoaderOptions = Omit<
  RouteDocumentLoadPorts,
  'acknowledgeOpenTarget' | 'releaseOpenTarget' | 'reportError'
> & {
  acknowledgeOpenTarget?: RouteDocumentLoadPorts['acknowledgeOpenTarget'];
  releaseOpenTarget?: RouteDocumentLoadPorts['releaseOpenTarget'];
  reportError?: (error: unknown) => void;
};

type RouteLeaveHandlerOptions = {
  routeFileLoader: Pick<RouteDocumentLoadCoordinator, 'dispose'>;
  hasActiveDocument: () => boolean;
  closeCurrentDocument: () => Promise<boolean>;
};

export function useRouteFileLoader({
  acknowledgeOpenTarget: acknowledge = acknowledgeOpenTarget,
  releaseOpenTarget: release = releaseOpenTarget,
  reportError = (error) => {
    console.error('Failed to handle route file load:', error);
  },
  ...ports
}: UseRouteFileLoaderOptions): RouteDocumentLoadCoordinator {
  const coordinator = createRouteDocumentLoadCoordinator({
    ...ports,
    acknowledgeOpenTarget: acknowledge,
    releaseOpenTarget: release,
    reportError,
  });
  onScopeDispose(() => {
    void coordinator.dispose();
  });
  return coordinator;
}

export function createRouteLeaveHandler({
  routeFileLoader,
  hasActiveDocument,
  closeCurrentDocument,
}: RouteLeaveHandlerOptions) {
  return async () => {
    if (!hasActiveDocument()) {
      await routeFileLoader.dispose();
      return true;
    }

    const canLeave = await closeCurrentDocument();
    if (canLeave) {
      await routeFileLoader.dispose();
    }
    return canLeave;
  };
}
