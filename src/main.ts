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

let restorationFailed = false;
let restoredActiveDocument = false;
if (typeof window !== "undefined" && "__TAURI_INTERNALS__" in window) {
  try {
    restoredActiveDocument = await applicationWorkspaceRuntime.restoreActiveDocument();
  } catch (error) {
    restorationFailed = true;
    console.error("Failed to restore the active document:", error);
  }
}

let unregisterRestorationExitGuard: () => void = () => undefined;
if (restorationFailed) {
  unregisterRestorationExitGuard = applicationWorkspaceRuntime.applicationExit.registerGuard(
    async () => {
      try {
        const restored = await applicationWorkspaceRuntime.restoreActiveDocument();
        if (restored) {
          await router.replace({ name: "table" });
          unregisterRestorationExitGuard();
          return null;
        }
        unregisterRestorationExitGuard();
        return inertExitPreparation();
      } catch (error) {
        console.error("Failed to recover the active document before exit:", error);
        return null;
      }
    },
  );
}

await router.isReady();
if (restoredActiveDocument && router.currentRoute.value.name === "home") {
  await router.replace({ name: "table" });
}

app.mount("#app");
void windowCloseRequests.start();

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
