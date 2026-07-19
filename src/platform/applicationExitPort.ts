import { getCurrentWindow } from '@tauri-apps/api/window';
import { relaunch } from '@tauri-apps/plugin-process';

import type {
  ApplicationExitExecutor,
  ApplicationExitIntent,
} from '@/application/applicationExitCoordinator';

export const tauriApplicationExitExecutor: ApplicationExitExecutor = {
  async execute(intent: ApplicationExitIntent): Promise<void> {
    if (intent === 'relaunch') {
      await relaunch();
      return;
    }
    await getCurrentWindow().destroy();
  },
};
