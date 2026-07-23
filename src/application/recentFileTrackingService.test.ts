import { describe, expect, it, vi } from 'vitest';

import { createRecentFilesService } from '@/application/recentFilesService';

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

describe('recent files tracking coordinator', () => {
  it('passes the captured document context through its tracking port and refreshes', async () => {
    const { service, port, reportFailure } = createService();

    service.queueRecentFileEntryUpdate({
      originalPath: '/original/book.xlsx',
      context: { documentId: '8', baseRevision: '4' },
    });
    await flushPromises();

    expect(port.addRecentFileWithThumbnail).toHaveBeenCalledWith(
      { documentId: '8', baseRevision: '4' },
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
      context: { documentId: '7', baseRevision: '3' },
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
      context: { documentId: '4', baseRevision: '2' },
    });
    await flushPromises();

    let disposed = false;
    const disposal = service.dispose().then(() => {
      disposed = true;
    });
    service.queueRecentFileEntryUpdate({
      context: { documentId: '5', baseRevision: '0' },
    });
    await Promise.resolve();
    expect(disposed).toBe(false);

    release();
    await disposal;
    expect(port.addRecentFileWithThumbnail).toHaveBeenCalledTimes(1);
    expect(port.getRecentFiles).not.toHaveBeenCalled();
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
});
