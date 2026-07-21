import { describe, expect, it, vi } from 'vitest';
import { createRouteDocumentLoadCoordinator } from '@/application/routeDocumentLoadCoordinator';

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((promiseResolve) => {
    resolve = promiseResolve;
  });
  return { promise, resolve };
}

async function flushPromises() {
  for (let index = 0; index < 8; index += 1) {
    await Promise.resolve();
  }
}

describe('routeDocumentLoadCoordinator', () => {
  it('cancels queued route file loads before they start', async () => {
    let routeFilePath: string | null = '/tmp/queued.xlsx';
    const releaseLoad = deferred<boolean>();
    const firstLoad = vi.fn(() => releaseLoad.promise);
    const secondLoad = vi.fn().mockResolvedValue(true);
    const coordinator = createCoordinator({
      getRouteFilePath: () => routeFilePath,
      loadFileFromPath: vi.fn()
        .mockImplementationOnce(firstLoad)
        .mockImplementationOnce(secondLoad),
    });

    coordinator.enqueue('/tmp/queued.xlsx');
    coordinator.enqueue('/tmp/queued.xlsx');
    await flushPromises();

    routeFilePath = null;
    coordinator.cancel();
    releaseLoad.resolve(true);
    await flushPromises();

    expect(firstLoad).toHaveBeenCalledTimes(1);
    expect(secondLoad).not.toHaveBeenCalled();
  });

  it('passes a continuation guard to in-flight route file loads', async () => {
    let routeFilePath: string | null = '/tmp/current.xlsx';
    const routeGuard: { current?: () => boolean } = {};
    const releaseLoad = deferred<boolean>();
    const loadFileFromPath = vi.fn((_filePath: string, guard: () => boolean) => {
      routeGuard.current = guard;
      return releaseLoad.promise;
    });
    const coordinator = createCoordinator({
      getRouteFilePath: () => routeFilePath,
      loadFileFromPath,
    });

    coordinator.enqueue('/tmp/current.xlsx');
    await flushPromises();

    expect(routeGuard.current?.()).toBe(true);
    routeFilePath = '/tmp/next.xlsx';
    expect(routeGuard.current?.()).toBe(false);

    releaseLoad.resolve(false);
    await flushPromises();
  });

  it('notifies in-flight route file loads synchronously when cancelled', async () => {
    let cancelled = false;
    const releaseLoad = deferred<boolean>();
    const loadFileFromPath = vi.fn((_filePath: string, guard: {
      onCancel: (handler: () => void) => void;
    }) => {
      guard.onCancel(() => {
        cancelled = true;
      });
      return releaseLoad.promise;
    });
    const coordinator = createCoordinator({ loadFileFromPath });

    coordinator.enqueue('/tmp/slow.xlsx');
    await flushPromises();
    coordinator.cancel();

    expect(cancelled).toBe(true);

    releaseLoad.resolve(false);
    await flushPromises();
  });

  it('does not reload the already loaded route file', async () => {
    const loadFileFromPath = vi.fn().mockResolvedValue(true);
    const coordinator = createCoordinator({
      getCurrentFilePath: () => '/tmp/book.xlsx',
      getRouteFilePath: () => '/tmp/book.xlsx',
      loadFileFromPath,
    });

    coordinator.enqueue('/tmp/book.xlsx');
    await flushPromises();
    coordinator.enqueue('/tmp/book.xlsx');
    await flushPromises();

    expect(loadFileFromPath).toHaveBeenCalledTimes(1);
  });

  it('reports editor state refresh failures for routes without a file', async () => {
    const error = new Error('status unavailable');
    const reportError = vi.fn();
    const coordinator = createCoordinator({
      getRouteFilePath: () => null,
      refreshEditorState: vi.fn().mockRejectedValue(error),
      reportError,
    });

    coordinator.enqueue(null);
    await flushPromises();

    expect(reportError).toHaveBeenCalledWith(error);
  });

  it('retains only the latest route while a load is in flight', async () => {
    let routeFilePath: string | null = '/tmp/first.xlsx';
    const releaseFirst = deferred<boolean>();
    const loadFileFromPath = vi.fn()
      .mockImplementationOnce(() => releaseFirst.promise)
      .mockResolvedValue(true);
    const coordinator = createCoordinator({
      getRouteFilePath: () => routeFilePath,
      loadFileFromPath,
    });

    coordinator.enqueue(routeFilePath);
    await flushPromises();
    for (let index = 0; index < 10_000; index += 1) {
      routeFilePath = `/tmp/queued-${index}.xlsx`;
      coordinator.enqueue(routeFilePath);
    }
    await flushPromises();

    expect(loadFileFromPath).toHaveBeenCalledTimes(1);

    releaseFirst.resolve(false);
    await flushPromises();

    expect(loadFileFromPath).toHaveBeenCalledTimes(2);
    expect(loadFileFromPath.mock.calls[1]?.[0]).toBe('/tmp/queued-9999.xlsx');
  });

  it('isolates cancellation and reporting failures from the latest load', async () => {
    let routeFilePath: string | null = '/tmp/first.xlsx';
    const releaseFirst = deferred<boolean>();
    const secondLoad = vi.fn().mockResolvedValue(true);
    const reportError = vi.fn(() => {
      throw new Error('reporter failed');
    });
    const coordinator = createCoordinator({
      getRouteFilePath: () => routeFilePath,
      reportError,
      loadFileFromPath: vi.fn()
        .mockImplementationOnce((_path, guard) => {
          guard.onCancel(() => {
            throw new Error('cancellation failed');
          });
          return releaseFirst.promise;
        })
        .mockImplementationOnce(secondLoad),
    });

    coordinator.enqueue(routeFilePath);
    await flushPromises();
    routeFilePath = '/tmp/second.xlsx';
    coordinator.enqueue(routeFilePath);
    releaseFirst.resolve(false);
    await flushPromises();

    expect(reportError).toHaveBeenCalledOnce();
    expect(secondLoad).toHaveBeenCalledOnce();
  });
});

type CoordinatorOverrides = Partial<Parameters<typeof createRouteDocumentLoadCoordinator>[0]>;

function createCoordinator(overrides: CoordinatorOverrides = {}) {
  return createRouteDocumentLoadCoordinator({
    getRouteFilePath: overrides.getRouteFilePath ?? (() => '/tmp/slow.xlsx'),
    getCurrentFilePath: overrides.getCurrentFilePath ?? (() => null),
    loadFileFromPath: overrides.loadFileFromPath ?? vi.fn().mockResolvedValue(true),
    refreshEditorState: overrides.refreshEditorState ?? vi.fn().mockResolvedValue(undefined),
    reportError: overrides.reportError ?? vi.fn(),
  });
}
