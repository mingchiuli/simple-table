import { getVersion } from '@tauri-apps/api/app';
import { openUrl } from '@tauri-apps/plugin-opener';
import { platform } from '@tauri-apps/plugin-os';
import { relaunch } from '@tauri-apps/plugin-process';
import { check, type Update } from '@tauri-apps/plugin-updater';
import { requestApplicationExit } from '@/application/applicationExitCoordinator';
import { invokeCommand } from '@/tauriInvoke';
import {
  useUpdateSessionStore,
  type UpdatePlatform,
} from '@/stores/updateSession';
import type { UpdateInfo } from '@/types';
import { appErrorMessage } from '@/utils/appError';

type DownloadEvent = Parameters<Update['downloadAndInstall']>[0] extends
  ((event: infer Event) => void) | undefined ? Event : never;

export type DesktopUpdateHandle = Pick<Update, 'version' | 'downloadAndInstall'>;

export type UpdatePort = {
  getVersion(): Promise<string>;
  platform(): string;
  checkDesktop(): Promise<DesktopUpdateHandle | null>;
  checkMobile(currentVersion: string): Promise<UpdateInfo | null>;
  openUrl(url: string): Promise<void>;
  relaunch(): Promise<void>;
  requestExit(relaunchApplication: () => Promise<void>): Promise<boolean>;
};

type UpdateCheckResult =
  | { platform: 'desktop'; appVersion: string; update: DesktopUpdateHandle | null }
  | { platform: 'mobile'; appVersion: string; update: UpdateInfo | null };

type UpdateRuntime = {
  currentVersionPromise: Promise<string> | null;
  updateCheckPromise: Promise<UpdateCheckResult> | null;
  downloadPromise: Promise<void> | null;
  desktopUpdate: DesktopUpdateHandle | null;
  operationToken: number;
};

type UpdateSessionStore = ReturnType<typeof useUpdateSessionStore>;

export type UpdateCoordinator = ReturnType<typeof createUpdateCoordinator>;

const coordinators = new WeakMap<object, UpdateCoordinator>();

const tauriUpdatePort: UpdatePort = {
  getVersion,
  platform,
  checkDesktop: check,
  checkMobile: (currentVersion) =>
    invokeCommand('check_update_mobile', { currentVersion }),
  openUrl,
  relaunch,
  requestExit: requestApplicationExit,
};

export function createUpdateCoordinator(store: UpdateSessionStore, port: UpdatePort) {
  const runtime: UpdateRuntime = {
    currentVersionPromise: null,
    updateCheckPromise: null,
    downloadPromise: null,
    desktopUpdate: null,
    operationToken: 0,
  };

  function synchronizePlatform() {
    store.setPlatform(normalizePlatform(port.platform()));
  }

  function initialize() {
    synchronizePlatform();
    const token = runtime.operationToken;
    void ensureCurrentVersion().catch((error) => {
      if (isCurrentOperation(token)) store.setErrorMessage(appErrorMessage(error));
    });
  }

  async function checkForUpdate() {
    synchronizePlatform();
    const token = beginOperation();
    store.beginCheck();

    try {
      runtime.updateCheckPromise ??= runUpdateCheck().finally(() => {
        runtime.updateCheckPromise = null;
      });
      const result = await runtime.updateCheckPromise;
      if (!isCurrentOperation(token)) return;

      if (result.platform === 'desktop') {
        runtime.desktopUpdate = result.update;
        store.applyDesktopCheck(result.appVersion, result.update?.version ?? null);
      } else {
        runtime.desktopUpdate = null;
        store.applyMobileCheck(result.appVersion, result.update);
      }
    } catch (error) {
      if (isCurrentOperation(token)) store.fail(appErrorMessage(error));
    }
  }

  async function downloadAndInstall() {
    if (store.status === 'ready') {
      await relaunchWhenReady(beginOperation());
      return;
    }
    if (runtime.downloadPromise) {
      await runtime.downloadPromise;
      return;
    }
    const update = runtime.desktopUpdate;
    if (!update) return;

    const token = beginOperation();
    store.beginDownload();
    runtime.downloadPromise = runDesktopDownload(update, token).finally(() => {
      runtime.downloadPromise = null;
    });
    await runtime.downloadPromise;
  }

  async function runDesktopDownload(update: DesktopUpdateHandle, token: number) {
    try {
      await update.downloadAndInstall((event: DownloadEvent) => {
        if (!isCurrentOperation(token)) return;
        switch (event.event) {
          case 'Started':
            store.setDownloadTotal(event.data.contentLength ?? 0);
            break;
          case 'Progress':
            store.addDownloadedBytes(event.data.chunkLength);
            break;
          case 'Finished':
            store.markReady();
            break;
        }
      });
      if (isCurrentOperation(token)) await relaunchWhenReady(token);
    } catch (error) {
      if (isCurrentOperation(token)) store.fail(appErrorMessage(error));
    }
  }

  async function relaunchWhenReady(token: number) {
    if (!isCurrentOperation(token)) return;
    await port.requestExit(port.relaunch);
  }

  async function handleMobileUpdate() {
    const info = store.mobileUpdateInfo;
    if (!info) return;

    const token = beginOperation();
    try {
      await port.openUrl(store.isAndroid && info.apkUrl ? info.apkUrl : info.releaseUrl);
    } catch (error) {
      if (isCurrentOperation(token)) store.fail(appErrorMessage(error));
    }
  }

  function reset() {
    if (runtime.downloadPromise) return;
    runtime.operationToken += 1;
    runtime.desktopUpdate = null;
    store.reset();
  }

  async function runUpdateCheck(): Promise<UpdateCheckResult> {
    const appVersion = await ensureCurrentVersion();
    if (store.isDesktop) {
      return { platform: 'desktop', appVersion, update: await port.checkDesktop() };
    }
    return {
      platform: 'mobile',
      appVersion,
      update: await port.checkMobile(appVersion),
    };
  }

  async function ensureCurrentVersion(): Promise<string> {
    if (store.currentVersion) return store.currentVersion;
    runtime.currentVersionPromise ??= port.getVersion()
      .then((version) => {
        store.setCurrentVersion(version);
        return version;
      })
      .finally(() => {
        runtime.currentVersionPromise = null;
      });
    return runtime.currentVersionPromise;
  }

  function beginOperation() {
    runtime.operationToken += 1;
    return runtime.operationToken;
  }

  function isCurrentOperation(token: number) {
    return token === runtime.operationToken;
  }

  synchronizePlatform();

  return {
    initialize,
    checkForUpdate,
    downloadAndInstall,
    handleMobileUpdate,
    reset,
  };
}

export function useUpdateCoordinator(): UpdateCoordinator {
  const store = useUpdateSessionStore();
  let coordinator = coordinators.get(store);
  if (!coordinator) {
    coordinator = createUpdateCoordinator(store, tauriUpdatePort);
    coordinators.set(store, coordinator);
  }
  return coordinator;
}

function normalizePlatform(value: string): UpdatePlatform {
  if (value === 'android') return 'android';
  if (value === 'ios') return 'ios';
  return 'desktop';
}
