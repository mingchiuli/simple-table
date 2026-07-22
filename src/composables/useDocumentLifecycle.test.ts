import { beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { useDocumentLifecycle } from "@/composables/useDocumentLifecycle";
import { useDocumentSessionStore } from "@/stores/documentSession";
import {
  createDocumentWorkspaceTestContext,
  type DocumentWorkspaceTestContext,
} from '@/test/documentWorkspaceTestContext';

vi.mock("element-plus", () => ({
  ElMessage: {
    error: vi.fn(),
  },
}));

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((promiseResolve, promiseReject) => {
    resolve = promiseResolve;
    reject = promiseReject;
  });
  return { promise, resolve, reject };
}

describe("useDocumentLifecycle", () => {
  let workspace: DocumentWorkspaceTestContext;

  beforeEach(() => {
    setActivePinia(createPinia());
    workspace = createDocumentWorkspaceTestContext();
    vi.clearAllMocks();
  });

  it("runs an action under a document lifecycle and releases it afterwards", async () => {
    const store = useDocumentSessionStore();
    const action = vi.fn().mockResolvedValue(undefined);
    const { runDocumentLifecycle } = workspace.run(() => useDocumentLifecycle());

    await expect(runDocumentLifecycle("loading", "Failed", action)).resolves.toBe("completed");

    expect(action).toHaveBeenCalledTimes(1);
    expect(store.lifecycle).toBe("idle");
  });

  it("does not run an action when another lifecycle is active", async () => {
    const store = useDocumentSessionStore();
    const action = vi.fn();
    const { runDocumentLifecycle } = workspace.run(() => useDocumentLifecycle());

    workspace.runtime.session.beginLifecycle('saving');

    await expect(runDocumentLifecycle("loading", "Failed", action)).resolves.toBe("skipped");

    expect(action).not.toHaveBeenCalled();
    expect(store.lifecycle).toBe("saving");
  });

  it("shows an error and releases lifecycle when the action fails", async () => {
    const elementPlus = await import("element-plus");
    const store = useDocumentSessionStore();
    const { runDocumentLifecycle } = workspace.run(() => useDocumentLifecycle());

    await expect(
      runDocumentLifecycle("saving", "Save failed", async () => {
        throw new Error("disk full");
      })
    ).resolves.toBe("failed");

    expect(elementPlus.ElMessage.error).toHaveBeenCalledWith("Save failed: Error: disk full");
    expect(store.lifecycle).toBe("idle");
  });

  it("waits for the active lifecycle when requested", async () => {
    const store = useDocumentSessionStore();
    const coordinator = workspace.runtime.session;
    const action = vi.fn().mockResolvedValue(undefined);
    const { runDocumentLifecycle } = workspace.run(() => useDocumentLifecycle());

    coordinator.beginLifecycle('saving');
    const runPromise = runDocumentLifecycle("loading", "Failed", action, {
      waitForIdle: true,
    });
    await Promise.resolve();

    expect(action).not.toHaveBeenCalled();

    coordinator.endLifecycle('saving');
    await expect(runPromise).resolves.toBe("completed");

    expect(action).toHaveBeenCalledTimes(1);
    expect(store.lifecycle).toBe("idle");
  });

  it("cancels a waiting lifecycle when its continuation guard expires", async () => {
    const store = useDocumentSessionStore();
    const coordinator = workspace.runtime.session;
    const action = vi.fn().mockResolvedValue(undefined);
    const { runDocumentLifecycle } = workspace.run(() => useDocumentLifecycle());
    let shouldContinue = true;

    coordinator.beginLifecycle('saving');
    const runPromise = runDocumentLifecycle("loading", "Failed", action, {
      waitForIdle: true,
      shouldContinue: () => shouldContinue,
    });
    await Promise.resolve();

    shouldContinue = false;
    coordinator.endLifecycle('saving');
    await expect(runPromise).resolves.toBe("skipped");

    expect(action).not.toHaveBeenCalled();
    expect(store.lifecycle).toBe("idle");
  });

  it("does not end a later lifecycle after an action released its own lifecycle early", async () => {
    const store = useDocumentSessionStore();
    const work = deferred<void>();
    const { runDocumentLifecycle } = workspace.run(() => useDocumentLifecycle());

    const runPromise = runDocumentLifecycle("loading", "Failed", async ({ release }) => {
      release();
      await work.promise;
    });
    await Promise.resolve();

    expect(store.lifecycle).toBe("idle");
    expect(workspace.runtime.session.beginLifecycle('saving')).toBe(true);

    work.resolve();
    await expect(runPromise).resolves.toBe("completed");

    expect(store.lifecycle).toBe("saving");
  });
});
