import { describe, expect, it, vi } from 'vitest';

import { createRecentFileTrackingService } from '@/application/recentFilesService';

const recentFile = {
  id: 'recent',
  path: '/tmp/book.xlsx',
  fileName: 'book.xlsx',
  lastOpened: 1,
  fileSize: 42,
  storageType: 'desktopPath' as const,
};

describe('recentFileTrackingService', () => {
  it('passes document context and original path through its port', async () => {
    const addRecentFileWithThumbnail = vi.fn().mockResolvedValue(recentFile);
    const reportFailure = vi.fn();
    const service = createRecentFileTrackingService(
      { addRecentFileWithThumbnail },
      reportFailure,
    );

    await expect(service.tryAddRecentFileWithThumbnail({
      originalPath: '/original/book.xlsx',
      context: { documentId: '8', baseRevision: '4' },
    })).resolves.toBe(true);

    expect(addRecentFileWithThumbnail).toHaveBeenCalledWith(
      { documentId: '8', baseRevision: '4' },
      '/original/book.xlsx',
    );
    expect(reportFailure).not.toHaveBeenCalled();
  });

  it('reports metadata failures without rejecting the main workflow', async () => {
    const error = new Error('metadata unavailable');
    const reportFailure = vi.fn();
    const service = createRecentFileTrackingService(
      { addRecentFileWithThumbnail: vi.fn().mockRejectedValue(error) },
      reportFailure,
    );

    await expect(service.tryAddRecentFileWithThumbnail({
      context: { documentId: '7', baseRevision: '3' },
    })).resolves.toBe(false);
    expect(reportFailure).toHaveBeenCalledWith(error);
  });

  it('contains refresh failures at the application boundary', async () => {
    const error = new Error('store unavailable');
    const reportFailure = vi.fn();
    const service = createRecentFileTrackingService(
      { addRecentFileWithThumbnail: vi.fn() },
      reportFailure,
    );

    await expect(service.tryRefreshRecentFiles(async () => {
      throw error;
    })).resolves.toBe(false);
    expect(reportFailure).toHaveBeenCalledWith(error);
  });
});
