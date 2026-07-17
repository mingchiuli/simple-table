import * as api from '@/api';
import {
  createRecentFileTrackingService,
  createRecentFilesService,
  type RecentFileTrackingService,
  type RecentFilesService,
} from '@/application/recentFilesService';
import { useRecentFilesStore } from '@/stores/recentFiles';

export type BoundRecentFilesService = RecentFilesService & RecentFileTrackingService;

const services = new WeakMap<object, BoundRecentFilesService>();

export function useRecentFilesService(): BoundRecentFilesService {
  const store = useRecentFilesStore();
  let service = services.get(store);
  if (!service) {
    const files = createRecentFilesService(store, api);
    const tracking = createRecentFileTrackingService(api, (error) => {
      console.warn('Failed to update recent file metadata', error);
    });
    service = { ...files, ...tracking };
    services.set(store, service);
  }
  return service;
}
