import { getCurrentWindow } from "@tauri-apps/api/window";
import { onMounted, onScopeDispose } from "vue";

type ApplicationExitGuard = () => Promise<boolean>;
type ExitAction = () => Promise<void>;

const exitGuards = new Set<ApplicationExitGuard>();
let activeExitRequest: Promise<boolean> | null = null;

export function useApplicationExitGuard(guard: ApplicationExitGuard) {
  exitGuards.add(guard);
  onScopeDispose(() => {
    exitGuards.delete(guard);
  });
}

export function requestApplicationExit(exit: ExitAction): Promise<boolean> {
  if (activeExitRequest) {
    return activeExitRequest;
  }

  activeExitRequest = runApplicationExit(exit).finally(() => {
    activeExitRequest = null;
  });
  return activeExitRequest;
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

async function runApplicationExit(exit: ExitAction): Promise<boolean> {
  const guards = Array.from(exitGuards).reverse();
  for (const guard of guards) {
    if (!(await guard())) {
      return false;
    }
  }

  await exit();
  return true;
}
