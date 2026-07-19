import "element-plus/dist/index.css";
import "element-plus/theme-chalk/dark/css-vars.css";
import "@/styles/base.css";
import "@/styles/platform.css";
import { createPinia } from "pinia";
import { createApp } from "vue";
import App from "@/App.vue";
import { createApplicationExitCoordinator } from '@/application/applicationExitCoordinator';
import { applicationExitCoordinatorKey } from '@/composables/useApplicationExit';
import { restoreActiveDocument } from "@/composables/restoreActiveDocument";
import { tauriApplicationExitExecutor } from '@/platform/applicationExitPort';
import router from "@/router";

const app = createApp(App);
const pinia = createPinia();
const applicationExitCoordinator = createApplicationExitCoordinator(
  tauriApplicationExitExecutor,
);

app.use(pinia);
app.use(router);
app.provide(applicationExitCoordinatorKey, applicationExitCoordinator);

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
