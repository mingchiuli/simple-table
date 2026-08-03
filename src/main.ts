import "element-plus/dist/index.css";
import "element-plus/theme-chalk/dark/css-vars.css";
import "@/styles/base.css";
import "@/styles/platform.css";
import { createPinia } from "pinia";
import { createApp } from "vue";
import App from "@/App.vue";
import { createWindowCloseRequestLifecycle } from '@/application/applicationExitCoordinator';
import { applicationExitCoordinatorKey } from '@/composables/useApplicationExit';
import {
  documentWorkspaceRuntimeKey,
} from '@/composables/documentWorkspaceRuntime';
import {
  applicationWorkspaceRuntimeKey,
  createApplicationWorkspaceRuntime,
} from '@/composables/applicationWorkspaceRuntime';
import router from "@/router";

const app = createApp(App);
const pinia = createPinia();
app.use(pinia);
app.use(router);
const applicationWorkspaceRuntime = createApplicationWorkspaceRuntime();
app.provide(applicationWorkspaceRuntimeKey, applicationWorkspaceRuntime);
app.provide(applicationExitCoordinatorKey, applicationWorkspaceRuntime.applicationExit);
app.provide(documentWorkspaceRuntimeKey, applicationWorkspaceRuntime.document);
const windowCloseRequests = createWindowCloseRequestLifecycle(
  applicationWorkspaceRuntime.applicationWindow,
  applicationWorkspaceRuntime.applicationExit,
);

const shouldRestoreActiveDocument = typeof window !== "undefined"
  && "__TAURI_INTERNALS__" in window;
let activeRestoration: Promise<boolean> | null = null;
let restorationRequired = shouldRestoreActiveDocument;
let unregisterRestorationExitGuard: () => void = () => undefined;
if (restorationRequired) {
  unregisterRestorationExitGuard = applicationWorkspaceRuntime.applicationExit.registerGuard(
    async () => {
      try {
        const restored = await restoreActiveDocumentOnce();
        if (restored) {
          await routeRestoredDocument();
          finishRestorationGuard();
          return null;
        }
        finishRestorationGuard();
        return inertExitPreparation();
      } catch (error) {
        console.error("Failed to recover the active document before exit:", error);
        throw error;
      }
    },
  );
}

void windowCloseRequests.start();
app.mount("#app");
if (restorationRequired) {
  void restoreActiveDocumentOnce().then(
    async (restored) => {
      if (restored) await routeRestoredDocument();
      finishRestorationGuard();
    },
    (error) => {
      console.error("Failed to restore the active document:", error);
    },
  );
}

if (import.meta.hot) {
  import.meta.hot.dispose(() => {
    windowCloseRequests.dispose();
    app.unmount();
    void applicationWorkspaceRuntime.dispose().catch((error) => {
      console.error('Failed to dispose application workspace:', error);
    });
  });
}

function inertExitPreparation() {
  return {
    commit: () => undefined,
    rollback: () => undefined,
  };
}

function restoreActiveDocumentOnce(): Promise<boolean> {
  if (activeRestoration) return activeRestoration;
  const restoration = applicationWorkspaceRuntime.restoreActiveDocument();
  activeRestoration = restoration;
  void restoration.then(
    () => {
      if (activeRestoration === restoration) activeRestoration = null;
    },
    () => {
      if (activeRestoration === restoration) activeRestoration = null;
    },
  );
  return restoration;
}

async function routeRestoredDocument() {
  await router.isReady();
  if (router.currentRoute.value.name === "home") {
    await router.replace({ name: "table" });
  }
}

function finishRestorationGuard() {
  if (!restorationRequired) return;
  restorationRequired = false;
  unregisterRestorationExitGuard();
}
