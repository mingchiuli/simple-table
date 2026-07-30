import * as api from '@/api';
import { restoreActiveDocument } from '@/application/activeDocumentRestoreCoordinator';
import {
  createApplicationExitCoordinator,
  type ApplicationExitCoordinator,
  type ApplicationWindowPort,
} from '@/application/applicationExitCoordinator';
import { runtimeRecentFile } from '@/application/recentFileProtocol';
import {
  createRecentFilesService,
  type RecentFilesService,
} from '@/application/recentFilesService';
import {
  createUpdateCoordinator,
  type UpdateCoordinator,
} from '@/application/updateCoordinator';
import type { DocumentLaunchCoordinator } from '@/application/documentLaunchCoordinator';
import type { RouteDocumentLoadCoordinator } from '@/application/routeDocumentLoadCoordinator';
import {
  createOperationCancellationSource,
  raceWithOperationCancellation,
  throwIfOperationCancellationFailed,
} from '@/application/operationCancellation';
import { drainAllSettled } from '@/application/asyncDrain';
import {
  createDocumentWorkspaceRuntime,
  type DocumentWorkspaceRuntime,
} from '@/composables/documentWorkspaceRuntime';
import { tauriApplicationWindowPort } from '@/platform/applicationExitPort';
import { tauriUpdatePort } from '@/platform/updatePort';
import { useRecentFilesStore } from '@/stores/recentFiles';
import { useUpdateSessionStore } from '@/stores/updateSession';
import { hasInjectionContext, inject, type InjectionKey } from 'vue';

type ApplicationWorkspaceRuntimeOptions = {
  applicationExit?: ApplicationExitCoordinator;
  applicationWindow?: ApplicationWindowPort;
  document?: DocumentWorkspaceRuntime;
};

export type ApplicationWorkspaceRuntime = {
  applicationExit: ApplicationExitCoordinator;
  applicationWindow: ApplicationWindowPort;
  document: DocumentWorkspaceRuntime;
  recentFiles: RecentFilesService;
  readonly updates: UpdateCoordinator;
  registerDocumentLaunch(coordinator: DocumentLaunchCoordinator): () => void;
  registerRouteDocumentLoad(coordinator: RouteDocumentLoadCoordinator): () => void;
  restoreActiveDocument(): Promise<boolean>;
  dispose(): Promise<void>;
};

export const applicationWorkspaceRuntimeKey: InjectionKey<ApplicationWorkspaceRuntime> =
  Symbol('application-workspace-runtime');

export function createApplicationWorkspaceRuntime(
  options: ApplicationWorkspaceRuntimeOptions = {},
): ApplicationWorkspaceRuntime {
  const applicationWindow = options.applicationWindow ?? tauriApplicationWindowPort;
  const applicationExit = options.applicationExit
    ?? createApplicationExitCoordinator(applicationWindow);
  const document = options.document ?? createDocumentWorkspaceRuntime();
  const applicationCancellation = createOperationCancellationSource();
  const recentFiles = createRecentFilesService(
    useRecentFilesStore(),
    {
      getRecentFiles: async () => (await api.getRecentFiles()).map(runtimeRecentFile),
      removeRecentFile: api.removeRecentFile,
      addRecentFileWithThumbnail: async (context, originalPath) => {
        runtimeRecentFile(await api.addRecentFileWithThumbnail(context, originalPath));
      },
    },
    (error) => console.warn('Failed to update recent file metadata', error),
    applicationCancellation.signal,
  );
  let updateCoordinator: UpdateCoordinator | null = null;
  let documentLaunch: DocumentLaunchCoordinator | null = null;
  const routeDocumentLoads = new Set<RouteDocumentLoadCoordinator>();
  let disposed = false;
  let disposal: Promise<void> | null = null;

  const runtime: ApplicationWorkspaceRuntime = {
    applicationExit,
    applicationWindow,
    document,
    recentFiles,
    restoreActiveDocument() {
      return document.runTask(
        ({ cancellation }) => restoreActiveDocument({
          isFrontendSessionInitialized: () =>
            document.document.data !== null || document.document.documentId !== null,
          loadActiveDocument: () => raceWithOperationCancellation(
            api.getActiveDocument(),
            cancellation,
          ),
          publishActiveDocument: (activeDocument) => {
            document.session.openDocumentResponse(
              activeDocument,
              activeDocument.document.path || null,
            );
          },
        }),
        false,
      );
    },
    get updates() {
      updateCoordinator ??= createUpdateCoordinator(
        useUpdateSessionStore(),
        tauriUpdatePort,
        {
          requestRelaunch: async () => {
            const result = await applicationExit.requestExit('relaunch');
            return result.status === 'executed' && result.intent === 'relaunch';
          },
        },
        applicationCancellation.signal,
      );
      if (disposed) void updateCoordinator.dispose();
      return updateCoordinator;
    },
    registerDocumentLaunch(coordinator) {
      if (disposed) {
        void drainAllSettled(
          [() => coordinator.dispose()],
          'Failed to dispose a late document launch coordinator',
        ).catch((error) => {
          console.error('Failed to dispose a late document launch coordinator', error);
        });
        return () => undefined;
      }
      if (documentLaunch && documentLaunch !== coordinator) {
        throw new Error('Application workspace already owns a document launch coordinator');
      }
      documentLaunch = coordinator;
      return () => {
        if (documentLaunch === coordinator) documentLaunch = null;
      };
    },
    registerRouteDocumentLoad(coordinator) {
      if (disposed) {
        void drainAllSettled(
          [() => coordinator.dispose()],
          'Failed to dispose a late route document load coordinator',
        ).catch((error) => {
          console.error('Failed to dispose a late route document load coordinator', error);
        });
        return () => undefined;
      }
      routeDocumentLoads.add(coordinator);
      return () => routeDocumentLoads.delete(coordinator);
    },
    dispose() {
      if (disposal) return disposal;
      disposed = true;
      const cancellationFailures = applicationCancellation.cancel();
      const ownedDocumentLaunch = documentLaunch;
      const ownedRouteDocumentLoads = [...routeDocumentLoads];
      disposal = drainAllSettled([
        () => throwIfOperationCancellationFailed(
          cancellationFailures,
          'Failed to notify every application workspace cancellation observer',
        ),
        () => ownedDocumentLaunch?.dispose(),
        ...ownedRouteDocumentLoads.map((coordinator) => () => coordinator.dispose()),
        () => applicationExit.dispose(),
        () => document.dispose(),
        () => recentFiles.dispose(),
        () => updateCoordinator?.dispose(),
      ], 'Failed to completely drain the application workspace');
      return disposal;
    },
  };
  return runtime;
}

export function useApplicationWorkspaceRuntime(): ApplicationWorkspaceRuntime {
  if (!hasInjectionContext()) {
    throw new Error('Application workspace runtime must be provided by the application root');
  }
  const runtime = inject(applicationWorkspaceRuntimeKey, null);
  if (!runtime) {
    throw new Error('Application workspace runtime must be provided by the application root');
  }
  return runtime;
}
