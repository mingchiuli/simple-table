import "element-plus/dist/index.css";
import "element-plus/theme-chalk/dark/css-vars.css";
import "@/styles/base.css";
import "@/styles/platform.css";
import { createPinia } from "pinia";
import { createApp } from "vue";
import App from "@/App.vue";
import { applicationExitCoordinatorKey } from '@/composables/useApplicationExit';
import {
  documentWorkspaceRuntimeKey,
} from '@/composables/documentWorkspaceRuntime';
import {
  applicationWorkspaceRuntimeKey,
  createApplicationWorkspaceRuntime,
} from '@/composables/applicationWorkspaceRuntime';
import { restoreActiveDocument } from "@/composables/restoreActiveDocument";
import { assertEditorResourcePolicyCompatibility } from '@/protocol/editorResourcePolicy';
import router from "@/router";

assertEditorResourcePolicyCompatibility();
const app = createApp(App);
const pinia = createPinia();
app.use(pinia);
app.use(router);
const applicationWorkspaceRuntime = createApplicationWorkspaceRuntime();
app.provide(applicationWorkspaceRuntimeKey, applicationWorkspaceRuntime);
app.provide(applicationExitCoordinatorKey, applicationWorkspaceRuntime.applicationExit);
app.provide(documentWorkspaceRuntimeKey, applicationWorkspaceRuntime.document);

let restoredActiveDocument = false;
if (typeof window !== "undefined" && "__TAURI_INTERNALS__" in window) {
  try {
    restoredActiveDocument = await restoreActiveDocument();
  } catch (error) {
    console.error("Failed to restore the active document:", error);
  }
}

await router.isReady();
if (restoredActiveDocument && router.currentRoute.value.name === "home") {
  await router.replace({ name: "table" });
}

app.mount("#app");

if (import.meta.hot) {
  import.meta.hot.dispose(() => {
    app.unmount();
    void applicationWorkspaceRuntime.dispose();
  });
}
