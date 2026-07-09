import { ElMessage } from "element-plus";
import {
  useDocumentSessionStore,
  type DocumentSessionLifecycle,
} from "@/stores/documentSession";

type ActiveDocumentLifecycle = Exclude<DocumentSessionLifecycle, "idle">;

export function useDocumentLifecycle() {
  const documentSessionStore = useDocumentSessionStore();

  async function runDocumentLifecycle(
    lifecycle: ActiveDocumentLifecycle,
    errorPrefix: string,
    action: () => Promise<void>
  ): Promise<boolean> {
    if (!documentSessionStore.beginLifecycle(lifecycle)) {
      return false;
    }
    try {
      await action();
    } catch (error) {
      ElMessage.error(`${errorPrefix}: ${error}`);
    } finally {
      documentSessionStore.endLifecycle(lifecycle);
    }
    return true;
  }

  return {
    runDocumentLifecycle,
  };
}
