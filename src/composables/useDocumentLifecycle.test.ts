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

    await expect(runDocumentLifecycle("loading", "Failed", action)).resolves.toBe(true);

    expect(action).toHaveBeenCalledTimes(1);
    expect(store.lifecycle).toBe("idle");
  });

  it("does not run an action when another lifecycle is active", async () => {
    const store = useDocumentSessionStore();
    const action = vi.fn();
    const { runDocumentLifecycle } = useDocumentLifecycle();

    store.beginLifecycle("saving");

    await expect(runDocumentLifecycle("loading", "Failed", action)).resolves.toBe(false);

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
    ).resolves.toBe(true);

    expect(elementPlus.ElMessage.error).toHaveBeenCalledWith("Save failed: Error: disk full");
    expect(store.lifecycle).toBe("idle");
  });
});
