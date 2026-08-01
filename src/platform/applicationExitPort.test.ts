import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({
  destroy: vi.fn(),
  listen: vi.fn(),
  relaunch: vi.fn(),
}));

vi.mock('@tauri-apps/api/event', () => ({ listen: mocks.listen }));
vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({ destroy: mocks.destroy }),
}));
vi.mock('@tauri-apps/plugin-process', () => ({ relaunch: mocks.relaunch }));

import {
  APPLICATION_CLOSE_REQUESTED_EVENT,
  tauriApplicationWindowPort,
} from '@/platform/applicationExitPort';

describe('tauri application exit port', () => {
  beforeEach(() => {
    vi.stubGlobal('window', { __TAURI_INTERNALS__: {} });
    mocks.destroy.mockReset().mockResolvedValue(undefined);
    mocks.listen.mockReset();
    mocks.relaunch.mockReset().mockResolvedValue(undefined);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('subscribes to the backend-owned close request event', async () => {
    const unlisten = vi.fn();
    const handler = vi.fn().mockResolvedValue(undefined);
    const eventHandlers: Array<() => void> = [];
    mocks.listen.mockImplementation(async (_event, registeredHandler) => {
      eventHandlers.push(registeredHandler as () => void);
      return unlisten;
    });

    await expect(
      tauriApplicationWindowPort.subscribeCloseRequested(handler),
    ).resolves.toBe(unlisten);
    expect(mocks.listen).toHaveBeenCalledWith(
      APPLICATION_CLOSE_REQUESTED_EVENT,
      expect.any(Function),
    );

    eventHandlers[0]?.();
    await Promise.resolve();
    expect(handler).toHaveBeenCalledOnce();
  });

  it('destroys the window only after the exit coordinator authorizes close', async () => {
    await tauriApplicationWindowPort.execute('close');

    expect(mocks.destroy).toHaveBeenCalledOnce();
    expect(mocks.relaunch).not.toHaveBeenCalled();
  });

  it('uses the process relaunch path without destroying the guarded window', async () => {
    await tauriApplicationWindowPort.execute('relaunch');

    expect(mocks.relaunch).toHaveBeenCalledOnce();
    expect(mocks.destroy).not.toHaveBeenCalled();
  });
});
