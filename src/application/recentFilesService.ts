import * as api from '@/api';
import { useRecentFilesStore } from '@/stores/recentFiles';
import type { RecentFile } from '@/types';

export type RecentFilesPort = {
  getRecentFiles(): Promise<RecentFile[]>;
  removeRecentFile(id: string): Promise<void>;
};

type RecentFilesState = Pick<
  ReturnType<typeof useRecentFilesStore>,
  'replaceFiles' | 'setLoading'
>;

type RecentFilesRuntime = {
  loadRequestId: number;
  activeLoadCount: number;
};

export type RecentFilesService = ReturnType<typeof createRecentFilesService>;

const services = new WeakMap<object, RecentFilesService>();

export function createRecentFilesService(
  store: RecentFilesState,
  port: RecentFilesPort,
) {
  const runtime: RecentFilesRuntime = {
    loadRequestId: 0,
    activeLoadCount: 0,
  };

  async function load() {
    const requestId = runtime.loadRequestId + 1;
    runtime.loadRequestId = requestId;
    runtime.activeLoadCount += 1;
    store.setLoading(true);
    try {
      const files = await port.getRecentFiles();
      if (requestId === runtime.loadRequestId) {
        store.replaceFiles(files);
      }
    } finally {
      runtime.activeLoadCount = Math.max(0, runtime.activeLoadCount - 1);
      store.setLoading(runtime.activeLoadCount > 0);
    }
  }

  async function remove(id: string) {
    await port.removeRecentFile(id);
    await load();
  }

  return { load, remove };
}

export function useRecentFilesService(): RecentFilesService {
  const store = useRecentFilesStore();
  let service = services.get(store);
  if (!service) {
    service = createRecentFilesService(store, api);
    services.set(store, service);
  }
  return service;
}
