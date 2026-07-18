import { createSearchSessionCoordinator } from '@/application/searchSessionCoordinator';
import { useSearchSessionStore } from '@/stores/searchSession';

type SearchSessionCoordinator = ReturnType<typeof createSearchSessionCoordinator>;

const coordinators = new WeakMap<object, SearchSessionCoordinator>();

export function useSearchSessionCoordinator() {
  const store = useSearchSessionStore();
  const existing = coordinators.get(store);
  if (existing) return existing;
  const coordinator = createSearchSessionCoordinator(store);
  coordinators.set(store, coordinator);
  return coordinator;
}
