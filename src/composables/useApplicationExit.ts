import { getCurrentWindow } from "@tauri-apps/api/window";
import { onMounted, onScopeDispose } from "vue";
import {
  registerApplicationExitGuard,
  requestApplicationExit,
  type ApplicationExitGuard,
} from "@/application/applicationExitCoordinator";

export { requestApplicationExit };

export function useApplicationExitGuard(guard: ApplicationExitGuard) {
  const unregister = registerApplicationExitGuard(guard);
  onScopeDispose(unregister);
}

export function useWindowCloseGuard() {
  let disposed = false;
  let unlisten: (() => void) | null = null;

  onMounted(async () => {
    if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) {
      return;
    }

    const appWindow = getCurrentWindow();
    try {
      const registeredUnlisten = await appWindow.onCloseRequested(async (event) => {
        event.preventDefault();
        try {
          await requestApplicationExit(() => appWindow.destroy());
        } catch (error) {
          console.error("Failed to close the application:", error);
        }
      });
      if (disposed) {
        registeredUnlisten();
      } else {
        unlisten = registeredUnlisten;
      }
    } catch (error) {
      console.error("Failed to register the application close guard:", error);
    }
  });

  onScopeDispose(() => {
    disposed = true;
    unlisten?.();
    unlisten = null;
  });
}
