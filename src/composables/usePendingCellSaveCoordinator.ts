import { createPendingCellSaveCoordinator } from '@/application/pendingCellSaveCoordinator';
import { usePendingCellSavesStore } from '@/stores/pendingCellSaves';

type PendingCellSaveCoordinator = ReturnType<typeof createPendingCellSaveCoordinator>;

const coordinators = new WeakMap<object, PendingCellSaveCoordinator>();

export function usePendingCellSaveCoordinator() {
  const store = usePendingCellSavesStore();
  const existing = coordinators.get(store);
  if (existing) return existing;
  const coordinator = createPendingCellSaveCoordinator(store);
  coordinators.set(store, coordinator);
  return coordinator;
}
