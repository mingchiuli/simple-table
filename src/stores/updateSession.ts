import type { UpdateInfo } from '@/types';

export type UpdateStatus =
  | 'idle'
  | 'checking'
  | 'available'
  | 'downloading'
  | 'ready'
  | 'error'
  | 'no-update';

export type UpdatePlatform = 'desktop' | 'android' | 'ios';

export type UpdateDownloadProgress = {
  downloaded: number;
  total: number;
  percentage: number;
};

const emptyProgress = (): UpdateDownloadProgress => ({
  downloaded: 0,
  total: 0,
  percentage: 0,
});

export const useUpdateSessionStore = defineStore('updateSession', {
  state: () => ({
    status: 'idle' as UpdateStatus,
    desktopUpdateVersion: null as string | null,
    mobileUpdateInfo: null as UpdateInfo | null,
    downloadProgress: emptyProgress(),
    errorMessage: null as string | null,
    currentVersion: '',
    updatePlatform: 'desktop' as UpdatePlatform,
  }),

  getters: {
    isChecking: (state) => state.status === 'checking',
    isDownloading: (state) => state.status === 'downloading',
    hasUpdate: (state) =>
      state.status === 'available'
      || state.status === 'downloading'
      || state.status === 'ready',
    isDesktop: (state) => state.updatePlatform === 'desktop',
    isAndroid: (state) => state.updatePlatform === 'android',
    isIOS: (state) => state.updatePlatform === 'ios',
  },

  actions: {
    setPlatform(platform: UpdatePlatform) {
      this.updatePlatform = platform;
    },

    setCurrentVersion(version: string) {
      this.currentVersion = version;
    },

    setErrorMessage(errorMessage: string) {
      this.errorMessage = errorMessage;
    },

    beginCheck() {
      this.status = 'checking';
      this.errorMessage = null;
    },

    applyDesktopCheck(appVersion: string, updateVersion: string | null) {
      this.currentVersion = appVersion;
      this.desktopUpdateVersion = updateVersion;
      this.mobileUpdateInfo = null;
      this.status = updateVersion ? 'available' : 'no-update';
    },

    applyMobileCheck(appVersion: string, update: UpdateInfo | null) {
      this.currentVersion = appVersion;
      this.mobileUpdateInfo = update;
      this.desktopUpdateVersion = null;
      this.status = update ? 'available' : 'no-update';
    },

    beginDownload() {
      this.status = 'downloading';
      this.downloadProgress = emptyProgress();
    },

    setDownloadTotal(total: number) {
      this.downloadProgress.total = total;
    },

    addDownloadedBytes(bytes: number) {
      this.downloadProgress.downloaded += bytes;
      if (this.downloadProgress.total > 0) {
        this.downloadProgress.percentage = Math.round(
          (this.downloadProgress.downloaded / this.downloadProgress.total) * 100,
        );
      }
    },

    markReady() {
      this.status = 'ready';
    },

    fail(errorMessage: string) {
      this.status = 'error';
      this.errorMessage = errorMessage;
    },

    reset() {
      this.status = 'idle';
      this.desktopUpdateVersion = null;
      this.mobileUpdateInfo = null;
      this.downloadProgress = emptyProgress();
      this.errorMessage = null;
    },
  },
});
