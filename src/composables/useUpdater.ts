import { storeToRefs } from 'pinia';
import { computed, onMounted } from 'vue';
import { useUpdateCoordinator } from '@/application/updateCoordinator';
import { useUpdateSessionStore } from '@/stores/updateSession';

export type { UpdateInfo } from '@/types';
export type { UpdateStatus } from '@/stores/updateSession';

export function useUpdater() {
  const session = useUpdateSessionStore();
  const coordinator = useUpdateCoordinator();
  onMounted(coordinator.initialize);
  const state = storeToRefs(session);
  const updateInfo = computed(() => session.desktopUpdateVersion
    ? { version: session.desktopUpdateVersion }
    : null);

  return {
    ...state,
    updateInfo,
    checkForUpdate: coordinator.checkForUpdate,
    downloadAndInstall: coordinator.downloadAndInstall,
    handleMobileUpdate: coordinator.handleMobileUpdate,
    reset: coordinator.reset,
  };
}
