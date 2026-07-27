import { listen } from '@tauri-apps/api/event';

import type { DocumentLaunchPort } from '@/application/documentLaunchCoordinator';
import {
  claimPendingOpenTarget,
  releaseOpenTarget,
} from '@/platform/loader';

export const tauriDocumentLaunchPort: DocumentLaunchPort = {
  onLaunchTargetAvailable: (handler) =>
    listen('deep-link-received', handler),
  claimPendingOpenTarget,
  releaseOpenTarget,
};
