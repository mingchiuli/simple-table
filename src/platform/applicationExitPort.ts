import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { relaunch } from '@tauri-apps/plugin-process';

import type {
  ApplicationExitIntent,
  ApplicationWindowPort,
} from '@/application/applicationExitCoordinator';

export const APPLICATION_CLOSE_REQUESTED_EVENT = 'application-close-requested';

export const tauriApplicationWindowPort: ApplicationWindowPort = {
  async subscribeCloseRequested(handler): Promise<() => void> {
    if (typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) {
      return () => undefined;
    }
    return listen(APPLICATION_CLOSE_REQUESTED_EVENT, () => {
      void handler();
    });
  },
  async execute(intent: ApplicationExitIntent): Promise<void> {
    if (intent === 'relaunch') {
      await relaunch();
      return;
    }
    await getCurrentWindow().destroy();
  },
};
