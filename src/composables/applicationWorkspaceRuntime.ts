import * as api from '@/api';
import { createApplicationExitCoordinator } from '@/application/applicationExitCoordinator';
import { runtimeRecentFile } from '@/application/recentFileProtocol';
import {
  createRecentFilesService,
  type RecentFilesService,
} from '@/application/recentFilesService';
import {
  createUpdateCoordinator,
  type UpdateCoordinator,
} from '@/application/updateCoordinator';
import {
  createDocumentWorkspaceRuntime,
  type DocumentWorkspaceRuntime,
} from '@/composables/documentWorkspaceRuntime';
import { tauriApplicationExitExecutor } from '@/platform/applicationExitPort';
import { tauriUpdatePort } from '@/platform/updatePort';
import { useRecentFilesStore } from '@/stores/recentFiles';
import { useUpdateSessionStore } from '@/stores/updateSession';
import type { ApplicationExitCoordinator } from '@/application/applicationExitCoordinator';
import { hasInjectionContext, inject, type InjectionKey } from 'vue';

type ApplicationWorkspaceRuntimeOptions = {
  applicationExit?: ApplicationExitCoordinator;
  document?: DocumentWorkspaceRuntime;
};

export type ApplicationWorkspaceRuntime = {
  applicationExit: ApplicationExitCoordinator;
  document: DocumentWorkspaceRuntime;
  recentFiles: RecentFilesService;
  readonly updates: UpdateCoordinator;
  dispose(): Promise<void>;
};

export const applicationWorkspaceRuntimeKey: InjectionKey<ApplicationWorkspaceRuntime> =
  Symbol('application-workspace-runtime');

export function createApplicationWorkspaceRuntime(
  options: ApplicationWorkspaceRuntimeOptions = {},
): ApplicationWorkspaceRuntime {
  const applicationExit = options.applicationExit
    ?? createApplicationExitCoordinator(tauriApplicationExitExecutor);
  const document = options.document ?? createDocumentWorkspaceRuntime();
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
  );
  let updateCoordinator: UpdateCoordinator | null = null;
  let disposed = false;
  let disposal: Promise<void> | null = null;

  const runtime: ApplicationWorkspaceRuntime = {
    applicationExit,
    document,
    recentFiles,
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
      );
      if (disposed) void updateCoordinator.dispose();
      return updateCoordinator;
    },
    dispose() {
      if (disposal) return disposal;
      disposed = true;
      disposal = Promise.all([
        applicationExit.dispose(),
        document.dispose(),
        recentFiles.dispose(),
        updateCoordinator?.dispose() ?? Promise.resolve(),
      ]).then(() => undefined);
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
