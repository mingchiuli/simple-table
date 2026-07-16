import { getVersion } from '@tauri-apps/api/app';
import { openUrl } from '@tauri-apps/plugin-opener';
import { platform } from '@tauri-apps/plugin-os';
import { relaunch } from '@tauri-apps/plugin-process';
import { check, type Update } from '@tauri-apps/plugin-updater';
import { requestApplicationExit } from '@/composables/useApplicationExit';
import { invokeCommand } from '@/tauriInvoke';
import type { UpdateInfo } from '@/types';
import { appErrorMessage } from '@/utils/appError';

export type UpdateStatus =
  | 'idle'
  | 'checking'
  | 'available'
  | 'downloading'
  | 'ready'
  | 'error'
  | 'no-update';

type UpdateCheckResult =
  | { platform: 'desktop'; appVersion: string; update: Update | null }
  | { platform: 'mobile'; appVersion: string; update: UpdateInfo | null };

export const useUpdateSessionStore = defineStore('updateSession', () => {
  const status = ref<UpdateStatus>('idle');
  const updateInfo = shallowRef<Update | null>(null);
  const mobileUpdateInfo = ref<UpdateInfo | null>(null);
  const downloadProgress = ref({ downloaded: 0, total: 0, percentage: 0 });
  const errorMessage = ref<string | null>(null);
  const currentVersion = ref('');
  let currentVersionPromise: Promise<string> | null = null;
  let updateCheckPromise: Promise<UpdateCheckResult> | null = null;
  let downloadPromise: Promise<void> | null = null;
  let operationToken = 0;

  const isChecking = computed(() => status.value === 'checking');
  const isDownloading = computed(() => status.value === 'downloading');
  const hasUpdate = computed(() =>
    status.value === 'available'
    || status.value === 'downloading'
    || status.value === 'ready'
  );
  const isDesktop = computed(() => {
    const osPlatform = platform();
    return osPlatform === 'macos' || osPlatform === 'windows' || osPlatform === 'linux';
  });
  const isAndroid = computed(() => platform() === 'android');
  const isIOS = computed(() => platform() === 'ios');

  function initialize() {
    const token = operationToken;
    void ensureCurrentVersion().catch((error) => {
      if (isCurrentOperation(token)) errorMessage.value = appErrorMessage(error);
    });
  }

  async function checkForUpdate() {
    const token = beginOperation();
    status.value = 'checking';
    errorMessage.value = null;

    try {
      updateCheckPromise ??= runUpdateCheck().finally(() => {
        updateCheckPromise = null;
      });
      const result = await updateCheckPromise;
      if (!isCurrentOperation(token)) return;

      currentVersion.value = result.appVersion;
      if (result.platform === 'desktop') {
        updateInfo.value = result.update;
        mobileUpdateInfo.value = null;
      } else {
        mobileUpdateInfo.value = result.update;
        updateInfo.value = null;
      }
      status.value = result.update ? 'available' : 'no-update';
    } catch (error) {
      if (!isCurrentOperation(token)) return;
      status.value = 'error';
      errorMessage.value = appErrorMessage(error);
    }
  }

  async function downloadAndInstall() {
    if (status.value === 'ready') {
      await relaunchWhenReady(beginOperation());
      return;
    }
    if (downloadPromise) {
      await downloadPromise;
      return;
    }
    const update = updateInfo.value;
    if (!update) return;

    const token = beginOperation();
    status.value = 'downloading';
    downloadProgress.value = { downloaded: 0, total: 0, percentage: 0 };
    downloadPromise = runDesktopDownload(update, token).finally(() => {
      downloadPromise = null;
    });
    await downloadPromise;
  }

  async function runDesktopDownload(update: Update, token: number) {
    try {
      await update.downloadAndInstall((event) => {
        if (!isCurrentOperation(token)) return;
        switch (event.event) {
          case 'Started':
            downloadProgress.value.total = event.data.contentLength ?? 0;
            break;
          case 'Progress':
            downloadProgress.value.downloaded += event.data.chunkLength;
            if (downloadProgress.value.total > 0) {
              downloadProgress.value.percentage = Math.round(
                (downloadProgress.value.downloaded / downloadProgress.value.total) * 100
              );
            }
            break;
          case 'Finished':
            status.value = 'ready';
            break;
        }
      });
      if (isCurrentOperation(token)) await relaunchWhenReady(token);
    } catch (error) {
      if (!isCurrentOperation(token)) return;
      status.value = 'error';
      errorMessage.value = appErrorMessage(error);
    }
  }

  async function relaunchWhenReady(token: number) {
    if (!isCurrentOperation(token)) return;
    await requestApplicationExit(relaunch);
  }

  async function handleMobileUpdate() {
    const info = mobileUpdateInfo.value;
    if (!info) return;

    const token = beginOperation();
    try {
      if (isAndroid.value && info.apkUrl) await openUrl(info.apkUrl);
      else await openUrl(info.releaseUrl);
    } catch (error) {
      if (!isCurrentOperation(token)) return;
      status.value = 'error';
      errorMessage.value = appErrorMessage(error);
    }
  }

  function reset() {
    if (downloadPromise) return;
    operationToken += 1;
    status.value = 'idle';
    updateInfo.value = null;
    mobileUpdateInfo.value = null;
    downloadProgress.value = { downloaded: 0, total: 0, percentage: 0 };
    errorMessage.value = null;
  }

  async function runUpdateCheck(): Promise<UpdateCheckResult> {
    const appVersion = await ensureCurrentVersion();
    if (isDesktop.value) {
      return { platform: 'desktop', appVersion, update: await check() };
    }
    return {
      platform: 'mobile',
      appVersion,
      update: await invokeCommand('check_update_mobile', { currentVersion: appVersion }),
    };
  }

  async function ensureCurrentVersion(): Promise<string> {
    if (currentVersion.value) return currentVersion.value;
    currentVersionPromise ??= getVersion()
      .then((version) => {
        currentVersion.value = version;
        return version;
      })
      .finally(() => {
        currentVersionPromise = null;
      });
    return currentVersionPromise;
  }

  function beginOperation() {
    operationToken += 1;
    return operationToken;
  }

  function isCurrentOperation(token: number) {
    return token === operationToken;
  }

  return {
    status,
    updateInfo,
    mobileUpdateInfo,
    downloadProgress,
    errorMessage,
    currentVersion,
    isChecking,
    isDownloading,
    hasUpdate,
    isDesktop,
    isAndroid,
    isIOS,
    initialize,
    checkForUpdate,
    downloadAndInstall,
    handleMobileUpdate,
    reset,
  };
});
