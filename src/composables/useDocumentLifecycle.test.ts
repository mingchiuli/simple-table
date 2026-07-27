import { beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { useDocumentLifecycle } from "@/composables/useDocumentLifecycle";
import { useDocumentSessionStore } from "@/stores/documentSession";
import {
  createDocumentWorkspaceTestContext,
  type DocumentWorkspaceTestContext,
} from '@/test/documentWorkspaceTestContext';

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

    await expect(runDocumentLifecycle("loading", action)).resolves.toBe("completed");

    expect(action).toHaveBeenCalledTimes(1);
    expect(store.lifecycle).toBe("idle");
  });

  it("does not run an action when another lifecycle is active", async () => {
    const store = useDocumentSessionStore();
    const action = vi.fn();
    const { runDocumentLifecycle } = workspace.run(() => useDocumentLifecycle());

    workspace.runtime.session.beginLifecycle('saving');

    await expect(runDocumentLifecycle("loading", action)).resolves.toBe("skipped");

    expect(action).not.toHaveBeenCalled();
    expect(store.lifecycle).toBe("saving");
  });

  it("propagates an error and releases lifecycle when the action fails", async () => {
    const store = useDocumentSessionStore();
    const { runDocumentLifecycle } = workspace.run(() => useDocumentLifecycle());

    await expect(
      runDocumentLifecycle("saving", async () => {
        throw new Error("disk full");
      })
    ).rejects.toThrow("disk full");

    expect(store.lifecycle).toBe("idle");
  });

  it("waits for the active lifecycle when requested", async () => {
    const store = useDocumentSessionStore();
    const coordinator = workspace.runtime.session;
    const action = vi.fn().mockResolvedValue(undefined);
    const { runDocumentLifecycle } = workspace.run(() => useDocumentLifecycle());

    coordinator.beginLifecycle('saving');
    const runPromise = runDocumentLifecycle("loading", action, {
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
    const runPromise = runDocumentLifecycle("loading", action, {
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

    const runPromise = runDocumentLifecycle("loading", async ({ release }) => {
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

  it("keeps a retained lifecycle active until its lease is released", async () => {
    const store = useDocumentSessionStore();
    const { runDocumentLifecycle } = workspace.run(() => useDocumentLifecycle());
    let releaseLease: (() => void) | undefined;

    await expect(
      runDocumentLifecycle("closing", async ({ retain }) => {
        releaseLease = retain().release;
      })
    ).resolves.toBe("completed");

    expect(store.lifecycle).toBe("closing");
    expect(store.isInteractionLocked).toBe(true);

    releaseLease?.();
    expect(store.lifecycle).toBe("idle");
    expect(store.isInteractionLocked).toBe(false);
  });

  it("releases a retained lifecycle when the action fails", async () => {
    const store = useDocumentSessionStore();
    const { runDocumentLifecycle } = workspace.run(() => useDocumentLifecycle());

    await expect(
      runDocumentLifecycle("closing", async ({ retain }) => {
        retain();
        throw new Error("cannot prepare");
      })
    ).rejects.toThrow("cannot prepare");

    expect(store.lifecycle).toBe("idle");
    expect(store.isInteractionLocked).toBe(false);
  });
});
