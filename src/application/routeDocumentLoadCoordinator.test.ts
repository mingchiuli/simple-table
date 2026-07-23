import { describe, expect, it, vi } from 'vitest';
import { createRouteDocumentLoadCoordinator } from '@/application/routeDocumentLoadCoordinator';
import type { OperationCancellationSignal } from '@/application/operationCancellation';

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

  it('passes a cancellation signal to in-flight route file loads', async () => {
    let routeFilePath: string | null = '/tmp/current.xlsx';
    const routeSignal: { current?: OperationCancellationSignal } = {};
    const releaseLoad = deferred<boolean>();
    const loadFileFromPath = vi.fn((_filePath: string, signal: OperationCancellationSignal) => {
      routeSignal.current = signal;
      return releaseLoad.promise;
    });
    const coordinator = createCoordinator({
      getRouteFilePath: () => routeFilePath,
      loadFileFromPath,
    });

    coordinator.enqueue('/tmp/current.xlsx');
    await flushPromises();

    expect(routeSignal.current?.isCancelled()).toBe(false);
    routeFilePath = '/tmp/next.xlsx';
    expect(routeSignal.current?.isCancelled()).toBe(true);

    releaseLoad.resolve(false);
    await flushPromises();
  });

  it('notifies in-flight route file loads synchronously when cancelled', async () => {
    let cancelled = false;
    const releaseLoad = deferred<boolean>();
    const loadFileFromPath = vi.fn((_filePath: string, signal: OperationCancellationSignal) => {
      signal.onCancel(() => {
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
        .mockImplementationOnce((_path, signal: OperationCancellationSignal) => {
          signal.onCancel(() => {
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

  it('acknowledges a launch target only after its document opens', async () => {
    const load = deferred<boolean>();
    const acknowledgeOpenTarget = vi.fn().mockResolvedValue(undefined);
    const coordinator = createCoordinator({
      getRouteFilePath: () => '/tmp/claimed.xlsx',
      getRouteOpenTargetClaimId: () => 'claim-1',
      loadFileFromPath: vi.fn(() => load.promise),
      acknowledgeOpenTarget,
    });

    coordinator.enqueue('/tmp/claimed.xlsx', 'claim-1');
    await flushPromises();
    expect(acknowledgeOpenTarget).not.toHaveBeenCalled();

    load.resolve(true);
    await flushPromises();

    expect(acknowledgeOpenTarget).toHaveBeenCalledWith('claim-1');
  });

  it('releases a launch target when its document does not open', async () => {
    const releaseOpenTarget = vi.fn().mockResolvedValue(undefined);
    const coordinator = createCoordinator({
      getRouteFilePath: () => '/tmp/rejected.xlsx',
      getRouteOpenTargetClaimId: () => 'claim-rejected',
      loadFileFromPath: vi.fn().mockResolvedValue(false),
      releaseOpenTarget,
    });

    coordinator.enqueue('/tmp/rejected.xlsx', 'claim-rejected');
    await flushPromises();

    expect(releaseOpenTarget).toHaveBeenCalledWith('claim-rejected');
  });

  it('releases active and superseded claims while acknowledging only the latest load', async () => {
    let routeFilePath = '/tmp/first.xlsx';
    let routeClaimId = 'claim-first';
    const firstLoad = deferred<boolean>();
    const acknowledgeOpenTarget = vi.fn().mockResolvedValue(undefined);
    const releaseOpenTarget = vi.fn().mockResolvedValue(undefined);
    const coordinator = createCoordinator({
      getRouteFilePath: () => routeFilePath,
      getRouteOpenTargetClaimId: () => routeClaimId,
      loadFileFromPath: vi.fn()
        .mockImplementationOnce(() => firstLoad.promise)
        .mockResolvedValueOnce(true),
      acknowledgeOpenTarget,
      releaseOpenTarget,
    });

    coordinator.enqueue(routeFilePath, routeClaimId);
    await flushPromises();
    routeFilePath = '/tmp/second.xlsx';
    routeClaimId = 'claim-second';
    coordinator.enqueue(routeFilePath, routeClaimId);
    routeFilePath = '/tmp/latest.xlsx';
    routeClaimId = 'claim-latest';
    coordinator.enqueue(routeFilePath, routeClaimId);
    firstLoad.resolve(false);
    await flushPromises();

    expect(releaseOpenTarget).toHaveBeenCalledWith('claim-first');
    expect(releaseOpenTarget).toHaveBeenCalledWith('claim-second');
    expect(acknowledgeOpenTarget).toHaveBeenCalledWith('claim-latest');
  });

  it('waits for the active load and claim settlement during disposal', async () => {
    const load = deferred<boolean>();
    const settlement = deferred<void>();
    const releaseOpenTarget = vi.fn(() => settlement.promise);
    const coordinator = createCoordinator({
      getRouteFilePath: () => '/tmp/active.xlsx',
      getRouteOpenTargetClaimId: () => 'claim-active',
      loadFileFromPath: vi.fn(() => load.promise),
      releaseOpenTarget,
    });
    coordinator.enqueue('/tmp/active.xlsx', 'claim-active');
    await flushPromises();

    let disposed = false;
    const disposal = coordinator.dispose().then(() => { disposed = true; });
    await flushPromises();
    expect(disposed).toBe(false);

    load.resolve(false);
    await flushPromises();
    expect(releaseOpenTarget).toHaveBeenCalledWith('claim-active');
    expect(disposed).toBe(false);

    settlement.resolve();
    await disposal;
    expect(disposed).toBe(true);
  });

  it('rejects post-disposal loads while still releasing their claims', async () => {
    const loadFileFromPath = vi.fn().mockResolvedValue(true);
    const releaseOpenTarget = vi.fn().mockResolvedValue(undefined);
    const coordinator = createCoordinator({ loadFileFromPath, releaseOpenTarget });

    await coordinator.dispose();
    coordinator.enqueue('/tmp/late.xlsx', 'claim-late');
    await coordinator.waitForIdle();

    expect(loadFileFromPath).not.toHaveBeenCalled();
    expect(releaseOpenTarget).toHaveBeenCalledWith('claim-late');
  });

  it('retries transient claim settlement failures', async () => {
    const releaseOpenTarget = vi
      .fn()
      .mockRejectedValueOnce(new Error('temporary failure'))
      .mockResolvedValueOnce(undefined);
    const reportError = vi.fn();
    const coordinator = createCoordinator({
      getRouteFilePath: () => '/tmp/retry.xlsx',
      getRouteOpenTargetClaimId: () => 'claim-retry',
      loadFileFromPath: vi.fn().mockResolvedValue(false),
      releaseOpenTarget,
      reportError,
    });

    coordinator.enqueue('/tmp/retry.xlsx', 'claim-retry');
    await coordinator.waitForIdle();

    expect(releaseOpenTarget).toHaveBeenCalledTimes(2);
    expect(reportError).not.toHaveBeenCalled();
  });
});

type CoordinatorOverrides = Partial<Parameters<typeof createRouteDocumentLoadCoordinator>[0]>;

function createCoordinator(overrides: CoordinatorOverrides = {}) {
  return createRouteDocumentLoadCoordinator({
    getRouteFilePath: overrides.getRouteFilePath ?? (() => '/tmp/slow.xlsx'),
    getRouteOpenTargetClaimId: overrides.getRouteOpenTargetClaimId ?? (() => null),
    getCurrentFilePath: overrides.getCurrentFilePath ?? (() => null),
    loadFileFromPath: overrides.loadFileFromPath ?? vi.fn().mockResolvedValue(true),
    refreshEditorState: overrides.refreshEditorState ?? vi.fn().mockResolvedValue(undefined),
    acknowledgeOpenTarget: overrides.acknowledgeOpenTarget ?? vi.fn().mockResolvedValue(undefined),
    releaseOpenTarget: overrides.releaseOpenTarget ?? vi.fn().mockResolvedValue(undefined),
    reportError: overrides.reportError ?? vi.fn(),
  });
}
