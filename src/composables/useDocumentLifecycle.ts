import { ElMessage } from "element-plus";
import {
  useDocumentSessionStore,
  type DocumentSessionLifecycle,
} from "@/stores/documentSession";

type ActiveDocumentLifecycle = Exclude<DocumentSessionLifecycle, "idle">;
export type DocumentLifecycleRunStatus = "completed" | "failed" | "skipped";

export function useDocumentLifecycle() {
  const documentSessionStore = useDocumentSessionStore();

  async function runDocumentLifecycle(
    lifecycle: ActiveDocumentLifecycle,
    errorPrefix: string,
    action: () => Promise<void>
  ): Promise<DocumentLifecycleRunStatus> {
    if (!documentSessionStore.beginLifecycle(lifecycle)) {
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

  return {
    runDocumentLifecycle,
  };
}
