import { describe, expect, it, vi } from 'vitest';
import { createRouteLeaveHandler } from '@/composables/useRouteFileLoader';

describe('createRouteLeaveHandler', () => {
  it('does not cancel route file loading when route leave is rejected', async () => {
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

  it('cancels route file loading after route leave is accepted', async () => {
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

  it('cancels route file loading immediately when no document is active', async () => {
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
