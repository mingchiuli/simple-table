import { storeToRefs } from 'pinia';
import { onMounted } from 'vue';
import { useUpdateSessionStore } from '@/stores/updateSession';

export type { UpdateInfo } from '@/types';
export type { UpdateStatus } from '@/stores/updateSession';

export function useUpdater() {
  const session = useUpdateSessionStore();
  onMounted(session.initialize);
  const state = storeToRefs(session);

  return {
    ...state,
    checkForUpdate: session.checkForUpdate,
    downloadAndInstall: session.downloadAndInstall,
    handleMobileUpdate: session.handleMobileUpdate,
    reset: session.reset,
  };
}
