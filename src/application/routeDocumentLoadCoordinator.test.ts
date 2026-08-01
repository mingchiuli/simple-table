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
    const secondCancellationObserver = vi.fn();
    const cancellationFailure = new Error('cancellation failed');
    const reportError = vi.fn(() => {
      throw new Error('reporter failed');
    });
    const coordinator = createCoordinator({
      getRouteFilePath: () => routeFilePath,
      reportError,
      loadFileFromPath: vi.fn()
        .mockImplementationOnce((_path, signal: OperationCancellationSignal) => {
          signal.onCancel(() => {
            throw cancellationFailure;
          });
          signal.onCancel(secondCancellationObserver);
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
    expect(reportError).toHaveBeenCalledWith(cancellationFailure);
    expect(secondCancellationObserver).toHaveBeenCalledOnce();
    expect(secondLoad).toHaveBeenCalledOnce();
    await expect(coordinator.waitForIdle()).rejects.toMatchObject({
      name: 'AggregateError',
      message: 'Failed to completely drain route document loading',
      errors: [cancellationFailure],
    });
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

  it('acknowledges a launch target when its document is rejected', async () => {
    const acknowledgeOpenTarget = vi.fn().mockResolvedValue(undefined);
    const coordinator = createCoordinator({
      getRouteFilePath: () => '/tmp/rejected.xlsx',
      getRouteOpenTargetClaimId: () => 'claim-rejected',
      loadFileFromPath: vi.fn().mockResolvedValue(false),
      acknowledgeOpenTarget,
    });

    coordinator.enqueue('/tmp/rejected.xlsx', 'claim-rejected');
    await flushPromises();

    expect(acknowledgeOpenTarget).toHaveBeenCalledWith('claim-rejected');
  });

  it('consumes active and pending claims superseded by the latest load', async () => {
    let routeFilePath = '/tmp/first.xlsx';
    let routeClaimId = 'claim-first';
    const firstLoad = deferred<boolean>();
    const acknowledgeOpenTarget = vi.fn().mockResolvedValue(undefined);
    const coordinator = createCoordinator({
      getRouteFilePath: () => routeFilePath,
      getRouteOpenTargetClaimId: () => routeClaimId,
      loadFileFromPath: vi.fn()
        .mockImplementationOnce(() => firstLoad.promise)
        .mockResolvedValueOnce(true),
      acknowledgeOpenTarget,
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

    expect(acknowledgeOpenTarget).toHaveBeenCalledWith('claim-first');
    expect(acknowledgeOpenTarget).toHaveBeenCalledWith('claim-second');
    expect(acknowledgeOpenTarget).toHaveBeenCalledWith('claim-latest');
  });

  it('transfers a claim to its replacement load without settling it early', async () => {
    const firstLoad = deferred<boolean>();
    const acknowledgeOpenTarget = vi.fn().mockResolvedValue(undefined);
    const loadFileFromPath = vi.fn()
      .mockImplementationOnce(() => firstLoad.promise)
      .mockResolvedValueOnce(true);
    const coordinator = createCoordinator({
      getRouteFilePath: () => '/tmp/book.xlsx',
      getRouteOpenTargetClaimId: () => 'claim-shared',
      loadFileFromPath,
      acknowledgeOpenTarget,
    });

    coordinator.enqueue('/tmp/book.xlsx', 'claim-shared');
    await flushPromises();
    coordinator.enqueue('/tmp/book.xlsx', 'claim-shared');
    firstLoad.resolve(false);
    await coordinator.waitForIdle();

    expect(loadFileFromPath).toHaveBeenCalledTimes(2);
    expect(acknowledgeOpenTarget).toHaveBeenCalledOnce();
    expect(acknowledgeOpenTarget).toHaveBeenCalledWith('claim-shared');
  });

  it('waits for the active load and claim settlement during disposal', async () => {
    const load = deferred<boolean>();
    const settlement = deferred<void>();
    const acknowledgeOpenTarget = vi.fn(() => settlement.promise);
    const coordinator = createCoordinator({
      getRouteFilePath: () => '/tmp/active.xlsx',
      getRouteOpenTargetClaimId: () => 'claim-active',
      loadFileFromPath: vi.fn(() => load.promise),
      acknowledgeOpenTarget,
    });
    coordinator.enqueue('/tmp/active.xlsx', 'claim-active');
    await flushPromises();

    let disposed = false;
    const disposal = coordinator.dispose().then(() => { disposed = true; });
    await flushPromises();
    expect(disposed).toBe(false);

    load.resolve(false);
    await flushPromises();
    expect(acknowledgeOpenTarget).toHaveBeenCalledWith('claim-active');
    expect(disposed).toBe(false);

    settlement.resolve();
    await disposal;
    expect(disposed).toBe(true);
  });

  it('rejects post-disposal loads while still acknowledging their claims', async () => {
    const loadFileFromPath = vi.fn().mockResolvedValue(true);
    const acknowledgeOpenTarget = vi.fn().mockResolvedValue(undefined);
    const coordinator = createCoordinator({ loadFileFromPath, acknowledgeOpenTarget });

    await coordinator.dispose();
    coordinator.enqueue('/tmp/late.xlsx', 'claim-late');
    await coordinator.waitForIdle();

    expect(loadFileFromPath).not.toHaveBeenCalled();
    expect(acknowledgeOpenTarget).toHaveBeenCalledWith('claim-late');
  });

  it('retries transient claim settlement failures', async () => {
    const acknowledgeOpenTarget = vi
      .fn()
      .mockRejectedValueOnce(new Error('temporary failure'))
      .mockResolvedValueOnce(undefined);
    const reportError = vi.fn();
    const coordinator = createCoordinator({
      getRouteFilePath: () => '/tmp/retry.xlsx',
      getRouteOpenTargetClaimId: () => 'claim-retry',
      loadFileFromPath: vi.fn().mockResolvedValue(false),
      acknowledgeOpenTarget,
      reportError,
    });

    coordinator.enqueue('/tmp/retry.xlsx', 'claim-retry');
    await coordinator.waitForIdle();

    expect(acknowledgeOpenTarget).toHaveBeenCalledTimes(2);
    expect(reportError).not.toHaveBeenCalled();
  });

  it('reports exhausted claim settlement failures from its lifecycle drain', async () => {
    const settlementFailure = new Error('claim settlement failed');
    const acknowledgeOpenTarget = vi.fn().mockRejectedValue(settlementFailure);
    const reportError = vi.fn();
    const coordinator = createCoordinator({
      getRouteFilePath: () => '/tmp/failed-settlement.xlsx',
      getRouteOpenTargetClaimId: () => 'claim-failed-settlement',
      loadFileFromPath: vi.fn().mockResolvedValue(false),
      acknowledgeOpenTarget,
      reportError,
    });

    coordinator.enqueue('/tmp/failed-settlement.xlsx', 'claim-failed-settlement');

    await expect(coordinator.dispose()).rejects.toMatchObject({
      name: 'AggregateError',
      message: 'Failed to completely drain route document loading',
      errors: [settlementFailure],
    });
    expect(acknowledgeOpenTarget).toHaveBeenCalledTimes(3);
    expect(reportError).toHaveBeenCalledWith(settlementFailure);
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
    reportError: overrides.reportError ?? vi.fn(),
  });
}
