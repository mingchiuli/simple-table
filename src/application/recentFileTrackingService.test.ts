import { describe, expect, it, vi } from 'vitest';

import { createRecentFilesService } from '@/application/recentFilesService';
import { OperationCancelledError } from '@/application/operationCancellation';
import type { FileOperationReceipt } from '@/types/fileRuntime';

async function flushPromises() {
  for (let index = 0; index < 8; index += 1) await Promise.resolve();
}

function createService(overrides: Record<string, unknown> = {}) {
  const reportFailure = vi.fn();
  const port = {
    getRecentFiles: vi.fn().mockResolvedValue([]),
    removeRecentFile: vi.fn().mockResolvedValue(undefined),
    addRecentFileWithThumbnail: vi.fn().mockResolvedValue(undefined),
    ...overrides,
  };
  const service = createRecentFilesService(
    { replaceFiles: vi.fn(), setLoading: vi.fn() },
    port,
    reportFailure,
  );
  return { service, port, reportFailure };
}

function receipt(
  path: string,
  documentId: `${bigint}` = '7',
  revision: `${bigint}` = '3',
): FileOperationReceipt {
  return {
    kind: 'open',
    documentId,
    revision,
    path,
    fileName: path.split('/').at(-1) ?? path,
  };
}

describe('recent files tracking coordinator', () => {
  it('passes the immutable file receipt through its tracking port and refreshes', async () => {
    const { service, port, reportFailure } = createService();
    const bookReceipt = receipt('/tmp/book.xlsx', '8', '4');

    service.queueRecentFileEntryUpdate({
      originalPath: '/original/book.xlsx',
      receipt: bookReceipt,
    });
    await flushPromises();

    expect(port.addRecentFileWithThumbnail).toHaveBeenCalledWith(
      bookReceipt,
      '/original/book.xlsx',
    );
    expect(port.getRecentFiles).toHaveBeenCalledTimes(1);
    expect(reportFailure).not.toHaveBeenCalled();
  });

  it('contains tracking failures and still refreshes the projection', async () => {
    const error = new Error('metadata unavailable');
    const { service, port, reportFailure } = createService({
      addRecentFileWithThumbnail: vi.fn().mockRejectedValue(error),
    });

    service.queueRecentFileEntryUpdate({
      receipt: receipt('/tmp/book.xlsx'),
    });
    await flushPromises();

    expect(reportFailure).toHaveBeenCalledWith(error);
    expect(port.getRecentFiles).toHaveBeenCalledTimes(1);
  });

  it('contains explicit refresh failures at the application boundary', async () => {
    const error = new Error('store unavailable');
    const { service, reportFailure } = createService({
      getRecentFiles: vi.fn().mockRejectedValue(error),
    });

    await expect(service.refresh()).resolves.toBe(false);
    expect(reportFailure).toHaveBeenCalledWith(error);
  });

  it('invalidates queued tracking and drains active metadata work on disposal', async () => {
    let release!: () => void;
    const active = new Promise<void>((resolve) => {
      release = resolve;
    });
    const { service, port } = createService({
      addRecentFileWithThumbnail: vi.fn().mockReturnValue(active),
    });
    service.queueRecentFileEntryUpdate({
      receipt: receipt('/tmp/active.xlsx', '4', '2'),
    });
    await flushPromises();

    let disposed = false;
    const disposal = service.dispose().then(() => {
      disposed = true;
    });
    service.queueRecentFileEntryUpdate({
      receipt: receipt('/tmp/queued.xlsx', '5', '0'),
    });
    await Promise.resolve();
    expect(disposed).toBe(false);

    release();
    await disposal;
    expect(port.addRecentFileWithThumbnail).toHaveBeenCalledTimes(1);
    expect(port.getRecentFiles).not.toHaveBeenCalled();
  });

  it('retains updates for every distinct file while another update is active', async () => {
    const active = deferred<void>();
    const { service, port } = createService({
      addRecentFileWithThumbnail: vi.fn()
        .mockReturnValueOnce(active.promise)
        .mockResolvedValue(undefined),
    });
    const first = receipt('/tmp/first.xlsx', '1', '0');
    const second = receipt('/tmp/second.xlsx', '2', '0');
    const third = receipt('/tmp/third.xlsx', '3', '0');

    service.queueRecentFileEntryUpdate({ receipt: first });
    await flushPromises();
    service.queueRecentFileEntryUpdate({ receipt: second });
    service.queueRecentFileEntryUpdate({ receipt: third });
    active.resolve();
    await service.waitForIdle();

    expect(port.addRecentFileWithThumbnail.mock.calls).toEqual([
      [first, undefined],
      [second, undefined],
      [third, undefined],
    ]);
  });

  it('drains an admitted removal and skips its refresh after disposal starts', async () => {
    let release!: () => void;
    const removal = new Promise<void>((resolve) => { release = resolve; });
    const { service, port } = createService({
      removeRecentFile: vi.fn().mockReturnValue(removal),
    });
    const activeRemoval = service.remove('recent-1');
    await Promise.resolve();

    let disposed = false;
    const disposal = service.dispose().then(() => { disposed = true; });
    await Promise.resolve();
    expect(disposed).toBe(false);

    release();
    await Promise.all([activeRemoval, disposal]);
    expect(port.removeRecentFile).toHaveBeenCalledWith('recent-1');
    expect(port.getRecentFiles).not.toHaveBeenCalled();
    await expect(service.remove('recent-2')).resolves.toBeUndefined();
    expect(port.removeRecentFile).toHaveBeenCalledTimes(1);
  });

  it('abandons an unresolved list observation on disposal', async () => {
    const { service } = createService({
      getRecentFiles: vi.fn(() => new Promise(() => undefined)),
    });
    const load = service.load();
    await Promise.resolve();

    const disposal = service.dispose();

    await expect(load).rejects.toBeInstanceOf(OperationCancelledError);
    await expect(disposal).resolves.toBeUndefined();
  });
});

function deferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void;
  const promise = new Promise<T>((promiseResolve) => {
    resolve = promiseResolve;
  });
  return { promise, resolve };
}
