import {
  createRouteDocumentLoadCoordinator,
  type RouteDocumentLoadCoordinator,
  type RouteDocumentLoadPorts,
} from '@/application/routeDocumentLoadCoordinator';
import { acknowledgeOpenTarget, releaseOpenTarget } from '@/platform';

type UseRouteFileLoaderOptions = Omit<
  RouteDocumentLoadPorts,
  'acknowledgeOpenTarget' | 'releaseOpenTarget' | 'reportError'
> & {
  acknowledgeOpenTarget?: RouteDocumentLoadPorts['acknowledgeOpenTarget'];
  releaseOpenTarget?: RouteDocumentLoadPorts['releaseOpenTarget'];
  reportError?: (error: unknown) => void;
};

type RouteLeaveHandlerOptions = {
  routeFileLoader: Pick<RouteDocumentLoadCoordinator, 'cancel'>;
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
  return createRouteDocumentLoadCoordinator({
    ...ports,
    acknowledgeOpenTarget: acknowledge,
    releaseOpenTarget: release,
    reportError,
  });
}

export function createRouteLeaveHandler({
  routeFileLoader,
  hasActiveDocument,
  closeCurrentDocument,
}: RouteLeaveHandlerOptions) {
  return async () => {
    if (!hasActiveDocument()) {
      routeFileLoader.cancel();
      return true;
    }

    const canLeave = await closeCurrentDocument();
    if (canLeave) {
      routeFileLoader.cancel();
    }
    return canLeave;
  };
}
