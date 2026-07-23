import { describe, expect, it, vi } from 'vitest';
import {
  createUpdateCoordinator,
  type DesktopUpdateHandle,
  type UpdateExitPort,
  type UpdatePort,
  type UpdateSessionPort,
} from '@/application/updateCoordinator';
import type { MobileUpdateState, UpdatePlatform } from '@/types/updateRuntime';

class TestUpdateSession implements UpdateSessionPort {
  status = 'idle';
  currentVersion = '';
  mobileUpdateInfo: MobileUpdateState | null = null;
  updatePlatform: UpdatePlatform = 'desktop';
  updateVersion: string | null = null;
  downloaded = 0;
  total = 0;
  error: string | null = null;

  get isDesktop() { return this.updatePlatform === 'desktop'; }
  get isAndroid() { return this.updatePlatform === 'android'; }

  setPlatform(platform: UpdatePlatform) { this.updatePlatform = platform; }
  setCurrentVersion(version: string) { this.currentVersion = version; }
  setErrorMessage(message: string) { this.error = message; }
  beginCheck() { this.status = 'checking'; }
  applyDesktopCheck(appVersion: string, updateVersion: string | null) {
    this.currentVersion = appVersion;
    this.updateVersion = updateVersion;
    this.status = updateVersion ? 'available' : 'no-update';
  }
  applyMobileCheck(appVersion: string, update: MobileUpdateState | null) {
    this.currentVersion = appVersion;
    this.mobileUpdateInfo = update;
    this.status = update ? 'available' : 'no-update';
  }
  beginDownload() { this.status = 'downloading'; }
  setDownloadTotal(total: number) { this.total = total; }
  addDownloadedBytes(bytes: number) { this.downloaded += bytes; }
  markReady() { this.status = 'ready'; }
  fail(message: string) { this.status = 'error'; this.error = message; }
  reset() { this.status = 'idle'; }
}

describe('updateCoordinator', () => {
  it('runs desktop update work entirely through injected ports', async () => {
    const session = new TestUpdateSession();
    const requestRelaunch = vi.fn(async () => true);
    const port: UpdatePort = {
      getVersion: async () => '1.0.0',
      platform: () => 'macos',
      checkDesktop: async () => ({
        version: '1.1.0',
        async downloadAndInstall(onEvent) {
          onEvent?.({ event: 'Started', data: { contentLength: 8 } });
          onEvent?.({ event: 'Progress', data: { chunkLength: 8 } });
          onEvent?.({ event: 'Finished', data: {} });
        },
      }),
      checkMobile: async () => null,
      openUrl: async () => undefined,
    };
    const exit: UpdateExitPort = { requestRelaunch };
    const coordinator = createUpdateCoordinator(session, port, exit);

    await coordinator.checkForUpdate();
    await coordinator.downloadAndInstall();

    expect(session.currentVersion).toBe('1.0.0');
    expect(session.updateVersion).toBe('1.1.0');
    expect(session.total).toBe(8);
    expect(session.downloaded).toBe(8);
    expect(session.status).toBe('ready');
    expect(requestRelaunch).toHaveBeenCalledOnce();
  });

  it('invalidates results and drains an active update check on disposal', async () => {
    const session = new TestUpdateSession();
    let release!: (value: DesktopUpdateHandle | null) => void;
    const check = new Promise<DesktopUpdateHandle | null>((resolve) => {
      release = resolve;
    });
    const coordinator = createUpdateCoordinator(session, {
      getVersion: async () => '1.0.0',
      platform: () => 'macos',
      checkDesktop: () => check,
      checkMobile: async () => null,
      openUrl: async () => undefined,
    }, { requestRelaunch: async () => true });
    const activeCheck = coordinator.checkForUpdate();
    await Promise.resolve();

    let disposed = false;
    const disposal = coordinator.dispose().then(() => {
      disposed = true;
    });
    await Promise.resolve();
    expect(disposed).toBe(false);

    release(null);
    await Promise.all([activeCheck, disposal]);
    expect(session.status).toBe('checking');
  });

  it('drains an active mobile URL launch on disposal', async () => {
    const session = new TestUpdateSession();
    session.mobileUpdateInfo = {
      version: '1.1.0',
      tagName: 'v1.1.0',
      releaseUrl: 'https://example.com/release',
      apkUrl: 'https://example.com/app.apk',
    };
    let release!: () => void;
    const open = new Promise<void>((resolve) => { release = resolve; });
    const openUrl = vi.fn().mockReturnValue(open);
    const coordinator = createUpdateCoordinator(session, {
      getVersion: async () => '1.0.0',
      platform: () => 'android',
      checkDesktop: async () => null,
      checkMobile: async () => null,
      openUrl,
    }, { requestRelaunch: async () => true });

    const launch = coordinator.handleMobileUpdate();
    await Promise.resolve();
    let disposed = false;
    const disposal = coordinator.dispose().then(() => { disposed = true; });
    await Promise.resolve();
    expect(disposed).toBe(false);

    release();
    await Promise.all([launch, disposal]);
    expect(openUrl).toHaveBeenCalledWith('https://example.com/app.apk');
    await coordinator.handleMobileUpdate();
    expect(openUrl).toHaveBeenCalledTimes(1);
  });
});
