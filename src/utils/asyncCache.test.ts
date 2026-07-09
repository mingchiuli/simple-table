import { describe, expect, it, vi } from "vitest";
import { createAsyncCache } from "@/utils/asyncCache";

describe("asyncCache", () => {
  it("shares a successful load across callers", async () => {
    const load = vi.fn(async () => ({ value: 1 }));
    const cache = createAsyncCache(load);

    const [first, second] = await Promise.all([cache.get(), cache.get()]);

    expect(first).toEqual({ value: 1 });
    expect(second).toBe(first);
    expect(load).toHaveBeenCalledTimes(1);
  });

  it("clears failed loads so callers can retry", async () => {
    const load = vi
      .fn<() => Promise<string>>()
      .mockRejectedValueOnce(new Error("temporary failure"))
      .mockResolvedValueOnce("ready");
    const cache = createAsyncCache(load);

    await expect(cache.get()).rejects.toThrow("temporary failure");
    await expect(cache.get()).resolves.toBe("ready");
    expect(load).toHaveBeenCalledTimes(2);
  });

  it("can be cleared explicitly", async () => {
    const load = vi.fn(async () => Symbol("loaded"));
    const cache = createAsyncCache(load);

    const first = await cache.get();
    cache.clear();
    const second = await cache.get();

    expect(second).not.toBe(first);
    expect(load).toHaveBeenCalledTimes(2);
  });
});
