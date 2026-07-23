import { ElMessage } from "element-plus";
import type { DocumentSessionLifecycle } from '@/types/documentRuntime';
import { appErrorMessage } from "@/utils/appError";
import { useDocumentSessionCoordinator } from '@/composables/useDocumentSessionCoordinator';

type ActiveDocumentLifecycle = Exclude<DocumentSessionLifecycle, "idle">;
export type DocumentLifecycleRunStatus = "completed" | "failed" | "skipped";

type DocumentLifecycleController = {
  release: () => void;
  retain: () => DocumentLifecycleLease;
};

type DocumentLifecycleLease = {
  release: () => void;
};

type DocumentLifecycleOptions = {
  waitForIdle?: boolean;
  shouldContinue?: () => boolean;
};

export function useDocumentLifecycle() {
  const documentSessionCoordinator = useDocumentSessionCoordinator();

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
    let retained = false;
    let completed = false;
    const release = () => {
      if (released) return;
      released = true;
      documentSessionCoordinator.endLifecycle(lifecycle);
    };
    const retain = (): DocumentLifecycleLease => {
      if (released) {
        throw new Error('Cannot retain a released document lifecycle');
      }
      retained = true;
      return { release };
    };
    try {
      await action({ release, retain });
      completed = true;
      return "completed";
    } catch (error) {
      ElMessage.error(`${errorPrefix}: ${appErrorMessage(error)}`);
      return "failed";
    } finally {
      if (!completed || !retained) release();
    }
  }

  async function acquireLifecycle(
    lifecycle: ActiveDocumentLifecycle,
    options: DocumentLifecycleOptions
  ): Promise<boolean> {
    if (!options.waitForIdle) {
      return documentSessionCoordinator.beginLifecycle(lifecycle);
    }

    while (options.shouldContinue?.() !== false) {
      if (documentSessionCoordinator.beginLifecycle(lifecycle)) {
        return true;
      }
      await documentSessionCoordinator.waitForInteractionIdle();
    }

    return false;
  }

  return {
    runDocumentLifecycle,
  };
}
