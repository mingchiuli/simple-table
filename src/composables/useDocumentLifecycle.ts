import { ElMessage } from "element-plus";
import {
  useDocumentSessionStore,
  type DocumentSessionLifecycle,
} from "@/stores/documentSession";

type ActiveDocumentLifecycle = Exclude<DocumentSessionLifecycle, "idle">;
export type DocumentLifecycleRunStatus = "completed" | "failed" | "skipped";

type DocumentLifecycleController = {
  release: () => void;
};

type DocumentLifecycleOptions = {
  waitForIdle?: boolean;
  shouldContinue?: () => boolean;
};

export function useDocumentLifecycle() {
  const documentSessionStore = useDocumentSessionStore();

  async function runDocumentLifecycle(
    lifecycle: ActiveDocumentLifecycle,
    errorPrefix: string,
    action: (controller: DocumentLifecycleController) => Promise<void>,
    options: DocumentLifecycleOptions = {}
  ): Promise<DocumentLifecycleRunStatus> {
    if (!(await acquireLifecycle(lifecycle, options))) {
      return "skipped";
    }
    let released = false;
    const release = () => {
      if (released) return;
      released = true;
      documentSessionStore.endLifecycle(lifecycle);
    };
    try {
      await action({ release });
      return "completed";
    } catch (error) {
      ElMessage.error(`${errorPrefix}: ${error}`);
      return "failed";
    } finally {
      release();
    }
  }

  async function acquireLifecycle(
    lifecycle: ActiveDocumentLifecycle,
    options: DocumentLifecycleOptions
  ): Promise<boolean> {
    if (!options.waitForIdle) {
      return documentSessionStore.beginLifecycle(lifecycle);
    }

    while (options.shouldContinue?.() !== false) {
      if (documentSessionStore.beginLifecycle(lifecycle)) {
        return true;
      }
      await documentSessionStore.waitForIdleLifecycle();
    }

    return false;
  }

  return {
    runDocumentLifecycle,
  };
}
