import { getVersion } from '@tauri-apps/api/app';
import { openUrl } from '@tauri-apps/plugin-opener';
import { platform } from '@tauri-apps/plugin-os';
import { relaunch } from '@tauri-apps/plugin-process';
import { check } from '@tauri-apps/plugin-updater';
import { requestApplicationExit } from '@/application/applicationExitCoordinator';
import type {
  DesktopDownloadEvent,
  DesktopUpdateHandle,
  UpdatePort,
} from '@/application/updateCoordinator';
import { invokeCommand } from '@/tauriInvoke';

export const tauriUpdatePort: UpdatePort = {
  getVersion,
  platform,
  async checkDesktop(): Promise<DesktopUpdateHandle | null> {
    const update = await check();
    if (!update) return null;
    return {
      version: update.version,
      downloadAndInstall: (onEvent) => update.downloadAndInstall((event) => {
        const mapped = mapDownloadEvent(event);
        if (mapped) onEvent?.(mapped);
      }),
    };
  },
  checkMobile: (currentVersion) =>
    invokeCommand('check_update_mobile', { currentVersion }),
  openUrl,
  relaunch,
  requestExit: requestApplicationExit,
};

type TauriDownloadEvent =
  | { event: 'Started'; data: { contentLength?: number } }
  | { event: 'Progress'; data: { chunkLength: number } }
  | { event: 'Finished' };

function mapDownloadEvent(event: TauriDownloadEvent): DesktopDownloadEvent | null {
  switch (event.event) {
    case 'Started':
      return { event: 'Started', data: { contentLength: event.data.contentLength } };
    case 'Progress':
      return { event: 'Progress', data: { chunkLength: event.data.chunkLength ?? 0 } };
    case 'Finished':
      return { event: 'Finished', data: {} };
    default:
      return null;
  }
}
