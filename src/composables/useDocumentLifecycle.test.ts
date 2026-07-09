import { beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { useDocumentLifecycle } from "@/composables/useDocumentLifecycle";
import { useDocumentSessionStore } from "@/stores/documentSession";

vi.mock("element-plus", () => ({
  ElMessage: {
    error: vi.fn(),
  },
}));

describe("useDocumentLifecycle", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
  });

  it("runs an action under a document lifecycle and releases it afterwards", async () => {
    const store = useDocumentSessionStore();
    const action = vi.fn().mockResolvedValue(undefined);
    const { runDocumentLifecycle } = useDocumentLifecycle();

    await expect(runDocumentLifecycle("loading", "Failed", action)).resolves.toBe("completed");

    expect(action).toHaveBeenCalledTimes(1);
    expect(store.lifecycle).toBe("idle");
  });

  it("does not run an action when another lifecycle is active", async () => {
    const store = useDocumentSessionStore();
    const action = vi.fn();
    const { runDocumentLifecycle } = useDocumentLifecycle();

    store.beginLifecycle("saving");

    await expect(runDocumentLifecycle("loading", "Failed", action)).resolves.toBe("skipped");

    expect(action).not.toHaveBeenCalled();
    expect(store.lifecycle).toBe("saving");
  });

  it("shows an error and releases lifecycle when the action fails", async () => {
    const elementPlus = await import("element-plus");
    const store = useDocumentSessionStore();
    const { runDocumentLifecycle } = useDocumentLifecycle();

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
    const action = vi.fn().mockResolvedValue(undefined);
    const { runDocumentLifecycle } = useDocumentLifecycle();

    store.beginLifecycle("saving");
    const runPromise = runDocumentLifecycle("loading", "Failed", action, {
      waitForIdle: true,
    });
    await Promise.resolve();

    expect(action).not.toHaveBeenCalled();

    store.endLifecycle("saving");
    await expect(runPromise).resolves.toBe("completed");

    expect(action).toHaveBeenCalledTimes(1);
    expect(store.lifecycle).toBe("idle");
  });

  it("cancels a waiting lifecycle when its continuation guard expires", async () => {
    const store = useDocumentSessionStore();
    const action = vi.fn().mockResolvedValue(undefined);
    const { runDocumentLifecycle } = useDocumentLifecycle();
    let shouldContinue = true;

    store.beginLifecycle("saving");
    const runPromise = runDocumentLifecycle("loading", "Failed", action, {
      waitForIdle: true,
      shouldContinue: () => shouldContinue,
    });
    await Promise.resolve();

    shouldContinue = false;
    store.endLifecycle("saving");
    await expect(runPromise).resolves.toBe("skipped");

    expect(action).not.toHaveBeenCalled();
    expect(store.lifecycle).toBe("idle");
  });
});
