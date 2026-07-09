import { ElMessage } from "element-plus";
import {
  useDocumentSessionStore,
  type DocumentSessionLifecycle,
} from "@/stores/documentSession";

type ActiveDocumentLifecycle = Exclude<DocumentSessionLifecycle, "idle">;
export type DocumentLifecycleRunStatus = "completed" | "failed" | "skipped";

type DocumentLifecycleOptions = {
  waitForIdle?: boolean;
  shouldContinue?: () => boolean;
};

export function useDocumentLifecycle() {
  const documentSessionStore = useDocumentSessionStore();

  async function runDocumentLifecycle(
    lifecycle: ActiveDocumentLifecycle,
    errorPrefix: string,
    action: () => Promise<void>,
    options: DocumentLifecycleOptions = {}
  ): Promise<DocumentLifecycleRunStatus> {
    if (!(await acquireLifecycle(lifecycle, options))) {
      return "skipped";
    }
    try {
      await action();
      return "completed";
    } catch (error) {
      ElMessage.error(`${errorPrefix}: ${error}`);
      return "failed";
    } finally {
      documentSessionStore.endLifecycle(lifecycle);
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
