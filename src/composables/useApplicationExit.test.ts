import { effectScope } from "vue";
import { describe, expect, it, vi } from "vitest";
import {
  requestApplicationExit,
  useApplicationExitGuard,
} from "@/composables/useApplicationExit";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((promiseResolve) => {
    resolve = promiseResolve;
  });
  return { promise, resolve };
}

describe("application exit coordination", () => {
  it("does not run the exit action when a guard rejects the request", async () => {
    const scope = effectScope();
    scope.run(() => {
      useApplicationExitGuard(vi.fn().mockResolvedValue(false));
    });
    const exit = vi.fn().mockResolvedValue(undefined);

    try {
      await expect(requestApplicationExit(exit)).resolves.toBe(false);
      expect(exit).not.toHaveBeenCalled();
    } finally {
      scope.stop();
    }
  });

  it("coalesces concurrent exit requests", async () => {
    const releaseGuard = deferred<boolean>();
    const scope = effectScope();
    scope.run(() => {
      useApplicationExitGuard(() => releaseGuard.promise);
    });
    const firstExit = vi.fn().mockResolvedValue(undefined);
    const secondExit = vi.fn().mockResolvedValue(undefined);

    try {
      const first = requestApplicationExit(firstExit);
      const second = requestApplicationExit(secondExit);
      releaseGuard.resolve(true);

      await expect(first).resolves.toBe(true);
      await expect(second).resolves.toBe(true);
      expect(firstExit).toHaveBeenCalledTimes(1);
      expect(secondExit).not.toHaveBeenCalled();
    } finally {
      scope.stop();
    }
  });
});
