import { describe, expect, it, vi } from 'vitest';
import { createRouteLeaveHandler } from '@/composables/useRouteFileLoader';

describe('createRouteLeaveHandler', () => {
  it('does not dispose route file loading when route leave is rejected', async () => {
    const routeFileLoader = { dispose: vi.fn().mockResolvedValue(undefined) };
    const closeCurrentDocument = vi.fn().mockResolvedValue(false);
    const leave = createRouteLeaveHandler({
      routeFileLoader,
      hasActiveDocument: () => true,
      closeCurrentDocument,
    });

    await expect(leave()).resolves.toBe(false);

    expect(closeCurrentDocument).toHaveBeenCalledTimes(1);
    expect(routeFileLoader.dispose).not.toHaveBeenCalled();
  });

  it('disposes route file loading after route leave is accepted', async () => {
    const routeFileLoader = { dispose: vi.fn().mockResolvedValue(undefined) };
    const closeCurrentDocument = vi.fn().mockResolvedValue(true);
    const leave = createRouteLeaveHandler({
      routeFileLoader,
      hasActiveDocument: () => true,
      closeCurrentDocument,
    });

    await expect(leave()).resolves.toBe(true);

    expect(routeFileLoader.dispose).toHaveBeenCalledTimes(1);
  });

  it('disposes route file loading immediately when no document is active', async () => {
    const routeFileLoader = { dispose: vi.fn().mockResolvedValue(undefined) };
    const closeCurrentDocument = vi.fn();
    const leave = createRouteLeaveHandler({
      routeFileLoader,
      hasActiveDocument: () => false,
      closeCurrentDocument,
    });

    await expect(leave()).resolves.toBe(true);

    expect(closeCurrentDocument).not.toHaveBeenCalled();
    expect(routeFileLoader.dispose).toHaveBeenCalledTimes(1);
  });
});
