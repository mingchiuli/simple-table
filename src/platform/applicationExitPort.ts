import { getCurrentWindow } from '@tauri-apps/api/window';
import { relaunch } from '@tauri-apps/plugin-process';

import type {
  ApplicationExitIntent,
  ApplicationWindowPort,
} from '@/application/applicationExitCoordinator';

export const tauriApplicationWindowPort: ApplicationWindowPort = {
  async subscribeCloseRequested(handler): Promise<() => void> {
    if (typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) {
      return () => undefined;
    }
    return getCurrentWindow().onCloseRequested((event) => {
      event.preventDefault();
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
