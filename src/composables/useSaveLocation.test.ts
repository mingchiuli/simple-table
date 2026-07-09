import { beforeEach, describe, expect, it, vi } from "vitest";
import { useSaveLocation } from "@/composables/useSaveLocation";

vi.mock("@/platform", () => ({
  discardSaveLocation: vi.fn(),
  pickSaveLocation: vi.fn(),
}));

describe("useSaveLocation", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("does not run the action when save location picking is cancelled", async () => {
    const platform = await import("@/platform");
    const action = vi.fn();
    vi.mocked(platform.pickSaveLocation).mockResolvedValue(null);

    const { withReservedSaveLocation } = useSaveLocation();

    await expect(withReservedSaveLocation("book.xlsx", action)).resolves.toBeNull();

    expect(action).not.toHaveBeenCalled();
    expect(platform.discardSaveLocation).not.toHaveBeenCalled();
  });

  it("discards the reserved location when the action exits without persisting", async () => {
    const platform = await import("@/platform");
    vi.mocked(platform.pickSaveLocation).mockResolvedValue("/tmp/reserved.xlsx");

    const { withReservedSaveLocation } = useSaveLocation();

    await expect(
      withReservedSaveLocation("book.xlsx", async ({ path }) => path)
    ).resolves.toBe("/tmp/reserved.xlsx");

    expect(platform.discardSaveLocation).toHaveBeenCalledWith("/tmp/reserved.xlsx");
  });

  it("discards the reserved location when the action fails before persisting", async () => {
    const platform = await import("@/platform");
    vi.mocked(platform.pickSaveLocation).mockResolvedValue("/tmp/reserved.xlsx");

    const { withReservedSaveLocation } = useSaveLocation();

    await expect(
      withReservedSaveLocation("book.xlsx", async () => {
        throw new Error("write failed");
      })
    ).rejects.toThrow("write failed");

    expect(platform.discardSaveLocation).toHaveBeenCalledWith("/tmp/reserved.xlsx");
  });

  it("keeps the original action error when reserved location cleanup also fails", async () => {
    const platform = await import("@/platform");
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => undefined);
    vi.mocked(platform.pickSaveLocation).mockResolvedValue("/tmp/reserved.xlsx");
    vi.mocked(platform.discardSaveLocation).mockRejectedValue(new Error("cleanup failed"));

    const { withReservedSaveLocation } = useSaveLocation();

    await expect(
      withReservedSaveLocation("book.xlsx", async () => {
        throw new Error("write failed");
      })
    ).rejects.toThrow("write failed");

    expect(platform.discardSaveLocation).toHaveBeenCalledWith("/tmp/reserved.xlsx");
    expect(consoleError).toHaveBeenCalledWith(
      "Failed to discard reserved save location after action error:",
      expect.any(Error)
    );

    consoleError.mockRestore();
  });

  it("reports cleanup failures when the action itself succeeds without persisting", async () => {
    const platform = await import("@/platform");
    vi.mocked(platform.pickSaveLocation).mockResolvedValue("/tmp/reserved.xlsx");
    vi.mocked(platform.discardSaveLocation).mockRejectedValue(new Error("cleanup failed"));

    const { withReservedSaveLocation } = useSaveLocation();

    await expect(
      withReservedSaveLocation("book.xlsx", async ({ path }) => path)
    ).rejects.toThrow("cleanup failed");
  });

  it("keeps the reserved location after it has been persisted", async () => {
    const platform = await import("@/platform");
    vi.mocked(platform.pickSaveLocation).mockResolvedValue("/tmp/saved.xlsx");

    const { withReservedSaveLocation } = useSaveLocation();

    await expect(
      withReservedSaveLocation("book.xlsx", async ({ markPersisted, path }) => {
        markPersisted();
        return path;
      })
    ).resolves.toBe("/tmp/saved.xlsx");

    expect(platform.discardSaveLocation).not.toHaveBeenCalled();
  });
});
