import type { MobileUpdateState, UpdatePlatform } from '@/types/updateRuntime';
import { appErrorMessage } from '@/utils/appError';
import {
  createOperationCancellationSource,
  neverCancelled,
  raceWithOperationCancellation,
  throwIfOperationCancellationFailed,
  type OperationCancellationSignal,
} from '@/application/operationCancellation';
import { drainAllSettled } from '@/application/asyncDrain';

export type DesktopDownloadEvent =
  | { event: 'Started'; data: { contentLength?: number } }
  | { event: 'Progress'; data: { chunkLength: number } }
  | { event: 'Finished'; data: Record<string, never> };

export type DesktopUpdateHandle = {
  version: string;
  downloadAndInstall(onEvent?: (event: DesktopDownloadEvent) => void): Promise<void>;
};

export type UpdatePort = {
  getVersion(): Promise<string>;
  platform(): string;
  checkDesktop(): Promise<DesktopUpdateHandle | null>;
  checkMobile(currentVersion: string): Promise<MobileUpdateState | null>;
  openUrl(url: string): Promise<void>;
};

export type UpdateExitPort = {
  requestRelaunch(): Promise<boolean>;
};

type UpdateCheckResult =
  | { platform: 'desktop'; appVersion: string; update: DesktopUpdateHandle | null }
  | { platform: 'mobile'; appVersion: string; update: MobileUpdateState | null };

type UpdateRuntime = {
  currentVersionPromise: Promise<string> | null;
  updateCheckPromise: Promise<UpdateCheckResult> | null;
  downloadPromise: Promise<void> | null;
  mobileOpenPromise: Promise<void> | null;
  exitPromise: Promise<void> | null;
  desktopUpdate: DesktopUpdateHandle | null;
  operationToken: number;
};

export type UpdateSessionPort = {
  status: string;
  currentVersion: string;
  mobileUpdateInfo: MobileUpdateState | null;
  isDesktop: boolean;
  isAndroid: boolean;
  setPlatform(platform: UpdatePlatform): void;
  setCurrentVersion(version: string): void;
  setErrorMessage(message: string): void;
  beginCheck(): void;
  applyDesktopCheck(appVersion: string, updateVersion: string | null): void;
  applyMobileCheck(appVersion: string, update: MobileUpdateState | null): void;
  beginDownload(): void;
  setDownloadTotal(total: number): void;
  addDownloadedBytes(bytes: number): void;
  markReady(): void;
  fail(message: string): void;
  reset(): void;
};

export type UpdateCoordinator = ReturnType<typeof createUpdateCoordinator>;

export function createUpdateCoordinator(
  store: UpdateSessionPort,
  port: UpdatePort,
  exit: UpdateExitPort,
  parentCancellation: OperationCancellationSignal = neverCancelled,
) {
  const runtime: UpdateRuntime = {
    currentVersionPromise: null,
    updateCheckPromise: null,
    downloadPromise: null,
    mobileOpenPromise: null,
    exitPromise: null,
    desktopUpdate: null,
    operationToken: 0,
  };
  const observationCancellation = createOperationCancellationSource();
  const unlinkParentCancellation = parentCancellation.onCancel(observationCancellation.cancel);
  let disposed = false;
  let disposal: Promise<void> | null = null;

  function synchronizePlatform() {
    store.setPlatform(normalizePlatform(port.platform()));
  }

  function initialize() {
    if (disposed) return;
    synchronizePlatform();
    const token = runtime.operationToken;
    void ensureCurrentVersion().catch((error) => {
      if (isCurrentOperation(token)) store.setErrorMessage(appErrorMessage(error));
    });
  }

  async function checkForUpdate() {
    if (disposed) return;
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
    if (disposed) return;
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
      await update.downloadAndInstall((event) => {
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
    runtime.exitPromise ??= exit.requestRelaunch()
      .then(() => undefined)
      .finally(() => {
        runtime.exitPromise = null;
      });
    await runtime.exitPromise;
  }

  async function handleMobileUpdate() {
    if (disposed) return;
    const info = store.mobileUpdateInfo;
    if (!info) return;
    if (runtime.mobileOpenPromise) {
      await runtime.mobileOpenPromise;
      return;
    }

    const token = beginOperation();
    runtime.mobileOpenPromise = raceWithOperationCancellation(
      () => port.openUrl(store.isAndroid && info.apkUrl ? info.apkUrl : info.releaseUrl),
      observationCancellation.signal,
    )
      .catch((error) => {
        if (isCurrentOperation(token)) store.fail(appErrorMessage(error));
      })
      .finally(() => {
        runtime.mobileOpenPromise = null;
      });
    await runtime.mobileOpenPromise;
  }

  function reset() {
    if (disposed || runtime.downloadPromise) return;
    runtime.operationToken += 1;
    runtime.desktopUpdate = null;
    store.reset();
  }

  async function runUpdateCheck(): Promise<UpdateCheckResult> {
    const appVersion = await ensureCurrentVersion();
    if (store.isDesktop) {
      return {
        platform: 'desktop',
        appVersion,
        update: await raceWithOperationCancellation(
          () => port.checkDesktop(),
          observationCancellation.signal,
        ),
      };
    }
    return {
      platform: 'mobile',
      appVersion,
      update: await raceWithOperationCancellation(
        () => port.checkMobile(appVersion),
        observationCancellation.signal,
      ),
    };
  }

  async function ensureCurrentVersion(): Promise<string> {
    if (store.currentVersion) return store.currentVersion;
    runtime.currentVersionPromise ??= raceWithOperationCancellation(
      () => port.getVersion(),
      observationCancellation.signal,
    )
      .then((version) => {
        if (!disposed) store.setCurrentVersion(version);
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
    return !disposed && token === runtime.operationToken;
  }

  function dispose(): Promise<void> {
    if (disposal) return disposal;
    disposed = true;
    runtime.operationToken += 1;
    runtime.desktopUpdate = null;
    const cancellationFailures = observationCancellation.cancel();
    unlinkParentCancellation();
    disposal = drainAllSettled([
      () => throwIfOperationCancellationFailed(
        cancellationFailures,
        'Failed to notify every update cancellation observer',
      ),
      waitForIdle,
    ], 'Failed to completely drain update coordination');
    return disposal;
  }

  async function waitForIdle(): Promise<void> {
    while (
      runtime.currentVersionPromise
      || runtime.updateCheckPromise
      || runtime.downloadPromise
      || runtime.mobileOpenPromise
      || runtime.exitPromise
    ) {
      await Promise.allSettled([
        ...(runtime.currentVersionPromise ? [runtime.currentVersionPromise] : []),
        ...(runtime.updateCheckPromise ? [runtime.updateCheckPromise] : []),
        ...(runtime.downloadPromise ? [runtime.downloadPromise] : []),
        ...(runtime.mobileOpenPromise ? [runtime.mobileOpenPromise] : []),
        ...(runtime.exitPromise ? [runtime.exitPromise] : []),
      ]);
    }
  }

  synchronizePlatform();

  return {
    initialize,
    checkForUpdate,
    downloadAndInstall,
    handleMobileUpdate,
    reset,
    waitForIdle,
    dispose,
  };
}

function normalizePlatform(value: string): UpdatePlatform {
  if (value === 'android') return 'android';
  if (value === 'ios') return 'ios';
  return 'desktop';
}
