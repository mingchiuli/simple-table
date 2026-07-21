import * as api from '@/api';
import {
  createRecentFilesService,
  type RecentFilesService,
} from '@/application/recentFilesService';
import { runtimeRecentFile } from '@/application/recentFileProtocol';
import { useRecentFilesStore } from '@/stores/recentFiles';

const services = new WeakMap<object, RecentFilesService>();

export function useRecentFilesService(): RecentFilesService {
  const store = useRecentFilesStore();
  let service = services.get(store);
  if (!service) {
    service = createRecentFilesService(
      store,
      {
        getRecentFiles: async () => (await api.getRecentFiles()).map(runtimeRecentFile),
        removeRecentFile: api.removeRecentFile,
        addRecentFileWithThumbnail: async (context, originalPath) => {
          runtimeRecentFile(await api.addRecentFileWithThumbnail(context, originalPath));
        },
      },
      (error) => console.warn('Failed to update recent file metadata', error),
    );
    services.set(store, service);
  }
  return service;
}
