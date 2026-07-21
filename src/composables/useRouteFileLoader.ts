import {
  createRouteDocumentLoadCoordinator,
  type RouteDocumentLoadCoordinator,
  type RouteDocumentLoadPorts,
} from '@/application/routeDocumentLoadCoordinator';

type UseRouteFileLoaderOptions = Omit<RouteDocumentLoadPorts, 'reportError'> & {
  reportError?: (error: unknown) => void;
};

type RouteLeaveHandlerOptions = {
  routeFileLoader: Pick<RouteDocumentLoadCoordinator, 'cancel'>;
  hasActiveDocument: () => boolean;
  closeCurrentDocument: () => Promise<boolean>;
};

export function useRouteFileLoader({
  reportError = (error) => {
    console.error('Failed to handle route file load:', error);
  },
  ...ports
}: UseRouteFileLoaderOptions): RouteDocumentLoadCoordinator {
  return createRouteDocumentLoadCoordinator({ ...ports, reportError });
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
