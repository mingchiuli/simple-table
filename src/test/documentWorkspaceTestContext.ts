import { createApp, type App } from 'vue';

import {
  createDocumentWorkspaceRuntime,
  documentWorkspaceRuntimeKey,
  type DocumentWorkspaceRuntime,
} from '@/composables/documentWorkspaceRuntime';
import {
  applicationWorkspaceRuntimeKey,
  createApplicationWorkspaceRuntime,
  type ApplicationWorkspaceRuntime,
} from '@/composables/applicationWorkspaceRuntime';

export type DocumentWorkspaceTestContext = {
  app: App;
  runtime: DocumentWorkspaceRuntime;
  run<T>(factory: () => T): T;
};

export type ApplicationWorkspaceTestContext = DocumentWorkspaceTestContext & {
  application: ApplicationWorkspaceRuntime;
};

export function createDocumentWorkspaceTestContext(): DocumentWorkspaceTestContext {
  const runtime = createDocumentWorkspaceRuntime();
  const app = createApp({});
  app.provide(documentWorkspaceRuntimeKey, runtime);
  return {
    app,
    runtime,
    run: (factory) => app.runWithContext(factory),
  };
}

export function createApplicationWorkspaceTestContext(): ApplicationWorkspaceTestContext {
  const context = createDocumentWorkspaceTestContext();
  const application = createApplicationWorkspaceRuntime({ document: context.runtime });
  context.app.provide(applicationWorkspaceRuntimeKey, application);
  return { ...context, application };
}
