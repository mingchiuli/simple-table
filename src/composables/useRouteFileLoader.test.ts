import { describe, expect, it, vi } from "vitest";
import {
  createRouteFileLoader,
  createRouteLeaveHandler,
} from "@/composables/useRouteFileLoader";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((promiseResolve, promiseReject) => {
    resolve = promiseResolve;
    reject = promiseReject;
  });
  return { promise, resolve, reject };
}

async function flushPromises() {
  for (let i = 0; i < 8; i += 1) {
    await Promise.resolve();
  }
}

describe("createRouteFileLoader", () => {
  it("cancels queued route file loads before they start", async () => {
    let routeFilePath: string | null = "/tmp/queued.xlsx";
    const releaseLoad = deferred<boolean>();
    const firstLoad = vi.fn(() => releaseLoad.promise);
    const secondLoad = vi.fn().mockResolvedValue(true);
    const loader = createRouteFileLoader({
      getRouteFilePath: () => routeFilePath,
      getCurrentFilePath: () => null,
      loadFileFromPath: vi.fn()
        .mockImplementationOnce(firstLoad)
        .mockImplementationOnce(secondLoad),
      refreshEditorState: vi.fn(),
      reportError: vi.fn(),
    });

    loader.enqueue("/tmp/queued.xlsx");
    loader.enqueue("/tmp/queued.xlsx");
    await flushPromises();

    routeFilePath = null;
    loader.cancel();
    releaseLoad.resolve(true);
    await flushPromises();

    expect(firstLoad).toHaveBeenCalledTimes(1);
    expect(secondLoad).not.toHaveBeenCalled();
  });

  it("passes a continuation guard to in-flight route file loads", async () => {
    let routeFilePath: string | null = "/tmp/current.xlsx";
    const routeGuard: { current?: () => boolean } = {};
    const releaseLoad = deferred<boolean>();
    const loadFileFromPath = vi.fn((_filePath: string, guard: () => boolean) => {
      routeGuard.current = guard;
      return releaseLoad.promise;
    });
    const loader = createRouteFileLoader({
      getRouteFilePath: () => routeFilePath,
      getCurrentFilePath: () => null,
      loadFileFromPath,
      refreshEditorState: vi.fn(),
      reportError: vi.fn(),
    });

    loader.enqueue("/tmp/current.xlsx");
    await flushPromises();

    if (!routeGuard.current) {
      throw new Error("expected continuation guard");
    }
    expect(routeGuard.current()).toBe(true);

    routeFilePath = "/tmp/next.xlsx";

    expect(routeGuard.current()).toBe(false);

    releaseLoad.resolve(false);
    await flushPromises();
  });

  it("notifies in-flight route file loads synchronously when cancelled", async () => {
    let cancelled = false;
    const releaseLoad = deferred<boolean>();
    const loadFileFromPath = vi.fn((_filePath: string, guard: { onCancel: (handler: () => void) => void }) => {
      guard.onCancel(() => {
        cancelled = true;
      });
      return releaseLoad.promise;
    });
    const loader = createRouteFileLoader({
      getRouteFilePath: () => "/tmp/slow.xlsx",
      getCurrentFilePath: () => null,
      loadFileFromPath,
      refreshEditorState: vi.fn(),
      reportError: vi.fn(),
    });

    loader.enqueue("/tmp/slow.xlsx");
    await flushPromises();

    loader.cancel();

    expect(cancelled).toBe(true);

    releaseLoad.resolve(false);
    await flushPromises();
  });

  it("does not reload the already loaded route file", async () => {
    const loadFileFromPath = vi.fn().mockResolvedValue(true);
    const loader = createRouteFileLoader({
      getRouteFilePath: () => "/tmp/book.xlsx",
      getCurrentFilePath: () => "/tmp/book.xlsx",
      loadFileFromPath,
      refreshEditorState: vi.fn(),
      reportError: vi.fn(),
    });

    loader.enqueue("/tmp/book.xlsx");
    await flushPromises();
    loader.enqueue("/tmp/book.xlsx");
    await flushPromises();

    expect(loadFileFromPath).toHaveBeenCalledTimes(1);
  });

  it("does not cancel route file loading when route leave is rejected", async () => {
    const routeFileLoader = { cancel: vi.fn() };
    const closeCurrentDocument = vi.fn().mockResolvedValue(false);
    const leave = createRouteLeaveHandler({
      routeFileLoader,
      hasActiveDocument: () => true,
      closeCurrentDocument,
    });

    await expect(leave()).resolves.toBe(false);

    expect(closeCurrentDocument).toHaveBeenCalledTimes(1);
    expect(routeFileLoader.cancel).not.toHaveBeenCalled();
  });

  it("cancels route file loading after route leave is accepted", async () => {
    const routeFileLoader = { cancel: vi.fn() };
    const closeCurrentDocument = vi.fn().mockResolvedValue(true);
    const leave = createRouteLeaveHandler({
      routeFileLoader,
      hasActiveDocument: () => true,
      closeCurrentDocument,
    });

    await expect(leave()).resolves.toBe(true);

    expect(routeFileLoader.cancel).toHaveBeenCalledTimes(1);
  });

  it("cancels route file loading immediately when no document is active", async () => {
    const routeFileLoader = { cancel: vi.fn() };
    const closeCurrentDocument = vi.fn();
    const leave = createRouteLeaveHandler({
      routeFileLoader,
      hasActiveDocument: () => false,
      closeCurrentDocument,
    });

    await expect(leave()).resolves.toBe(true);

    expect(closeCurrentDocument).not.toHaveBeenCalled();
    expect(routeFileLoader.cancel).toHaveBeenCalledTimes(1);
  });
});
