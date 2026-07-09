import { describe, expect, it, vi } from "vitest";
import { createDeepLinkLifecycle } from "@/composables/useDeepLinks";

type Dependencies = Parameters<typeof createDeepLinkLifecycle>[0];
type Unlisten = () => void;
type ListenHandler = (event: { payload: string }) => void;
type OpenUrlHandler = (urls: string[]) => void;

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
  await Promise.resolve();
  await Promise.resolve();
}

function testDependencies(overrides: Partial<Dependencies> = {}) {
  const listenHandlers: ListenHandler[] = [];
  const openUrlHandlers: OpenUrlHandler[] = [];
  const unlistenSingleInstance = vi.fn();
  const unlistenFileAssociation = vi.fn();
  const pushFilePath = vi.fn().mockResolvedValue(undefined);
  const reportError = vi.fn();
  const dependencies: Dependencies = {
    listen: vi.fn((_event, handler) => {
      listenHandlers.push(handler);
      return Promise.resolve(unlistenSingleInstance);
    }),
    onOpenUrl: vi.fn((handler) => {
      openUrlHandlers.push(handler);
      return Promise.resolve(unlistenFileAssociation);
    }),
    getCurrent: vi.fn().mockResolvedValue(null),
    pushFilePath,
    reportError,
    ...overrides,
  };

  return {
    dependencies,
    getListenHandler: () => listenHandlers.at(-1) ?? null,
    getOpenUrlHandler: () => openUrlHandlers.at(-1) ?? null,
    getListenHandlers: () => listenHandlers,
    pushFilePath,
    reportError,
    unlistenSingleInstance,
    unlistenFileAssociation,
  };
}

describe("createDeepLinkLifecycle", () => {
  it("routes deep links from startup, single-instance, and file-association sources", async () => {
    const setup = testDependencies({
      getCurrent: vi.fn().mockResolvedValue(["FILE:///Users/me/start.xlsx"]),
    });
    const lifecycle = createDeepLinkLifecycle(setup.dependencies);

    lifecycle.start();
    await flushPromises();
    setup.getListenHandler()?.({ payload: "file:///C:/Users/me/live.xlsx" });
    setup.getOpenUrlHandler()?.(["file://server/share/opened.xlsx"]);
    await flushPromises();

    expect(setup.pushFilePath).toHaveBeenCalledWith("/Users/me/start.xlsx");
    expect(setup.pushFilePath).toHaveBeenCalledWith("C:/Users/me/live.xlsx");
    expect(setup.pushFilePath).toHaveBeenCalledWith("//server/share/opened.xlsx");
  });

  it("cleans up listeners that resolve after the lifecycle has stopped", async () => {
    const pendingListen = deferred<Unlisten>();
    const unlistenSingleInstance = vi.fn();
    const setup = testDependencies({
      listen: vi.fn(() => pendingListen.promise),
    });
    const lifecycle = createDeepLinkLifecycle(setup.dependencies);

    lifecycle.start();
    lifecycle.stop();
    pendingListen.resolve(unlistenSingleInstance);
    await flushPromises();

    expect(unlistenSingleInstance).toHaveBeenCalledTimes(1);
  });

  it("cleans up the previous lifecycle when started twice", async () => {
    const setup = testDependencies();
    const lifecycle = createDeepLinkLifecycle(setup.dependencies);

    lifecycle.start();
    await flushPromises();
    lifecycle.start();
    await flushPromises();

    expect(setup.unlistenSingleInstance).toHaveBeenCalledTimes(1);
    expect(setup.unlistenFileAssociation).toHaveBeenCalledTimes(1);
  });

  it("ignores startup deep links from an old lifecycle", async () => {
    const firstStartup = deferred<string[] | null>();
    const secondStartup = deferred<string[] | null>();
    const getCurrent = vi
      .fn()
      .mockReturnValueOnce(firstStartup.promise)
      .mockReturnValueOnce(secondStartup.promise);
    const setup = testDependencies({ getCurrent });
    const lifecycle = createDeepLinkLifecycle(setup.dependencies);

    lifecycle.start();
    lifecycle.stop();
    lifecycle.start();
    firstStartup.resolve(["file:///old.xlsx"]);
    secondStartup.resolve(["file:///current.xlsx"]);
    await flushPromises();

    expect(setup.pushFilePath).toHaveBeenCalledTimes(1);
    expect(setup.pushFilePath).toHaveBeenCalledWith("/current.xlsx");
  });

  it("ignores stale event callbacks after a new lifecycle starts", async () => {
    const setup = testDependencies();
    const lifecycle = createDeepLinkLifecycle(setup.dependencies);

    lifecycle.start();
    await flushPromises();
    const firstListenHandler = setup.getListenHandlers()[0];
    lifecycle.start();
    await flushPromises();

    firstListenHandler?.({ payload: "file:///stale.xlsx" });
    setup.getListenHandler()?.({ payload: "file:///current.xlsx" });
    await flushPromises();

    expect(setup.pushFilePath).toHaveBeenCalledTimes(1);
    expect(setup.pushFilePath).toHaveBeenCalledWith("/current.xlsx");
  });

  it("reports cleanup errors and continues removing remaining listeners", async () => {
    const brokenUnlisten = vi.fn(() => {
      throw new Error("broken cleanup");
    });
    const healthyUnlisten = vi.fn();
    const reportError = vi.fn();
    const lifecycle = createDeepLinkLifecycle({
      listen: vi.fn(() => Promise.resolve(brokenUnlisten)),
      onOpenUrl: vi.fn(() => Promise.resolve(healthyUnlisten)),
      getCurrent: vi.fn().mockResolvedValue(null),
      pushFilePath: vi.fn().mockResolvedValue(undefined),
      reportError,
    });

    lifecycle.start();
    await flushPromises();
    lifecycle.stop();

    expect(brokenUnlisten).toHaveBeenCalledTimes(1);
    expect(healthyUnlisten).toHaveBeenCalledTimes(1);
    expect(reportError).toHaveBeenCalledWith(
      "Failed to clean up deep link listener:",
      expect.any(Error)
    );
  });
});
