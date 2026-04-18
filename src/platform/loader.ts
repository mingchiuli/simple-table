/**
 * Platform loader - Dynamic import factory with caching
 */
import { getPlatform } from '@/utils/platform';
import type { PlatformAPI } from './types';

let cachedAPI: PlatformAPI | null = null;
let loadingPromise: Promise<PlatformAPI> | null = null;

async function loadPlatformAPI(): Promise<PlatformAPI> {
  const platform = await getPlatform();

  switch (platform) {
    case 'android': {
      const { androidAPI } = await import('./android');
      return androidAPI;
    }
    case 'ios': {
      const { iosAPI } = await import('./ios');
      return iosAPI;
    }
    default: {
      // macos, windows, linux -> desktop
      const { desktopAPI } = await import('./desktop');
      return desktopAPI;
    }
  }
}

/**
 * Get the platform API (cached after first load)
 */
export async function getPlatformAPI(): Promise<PlatformAPI> {
  if (cachedAPI) {
    return cachedAPI;
  }

  if (!loadingPromise) {
    loadingPromise = loadPlatformAPI().then(api => {
      cachedAPI = api;
      return api;
    });
  }

  return loadingPromise;
}

// Convenience re-exports for file operations
export async function pickFile() {
  const api = await getPlatformAPI();
  return api.fileOps.pickFile();
}

export async function readFile(path: string) {
  const api = await getPlatformAPI();
  return api.fileOps.readFile(path);
}

export async function saveFile(path: string, bytes: number[]) {
  const api = await getPlatformAPI();
  return api.fileOps.saveFile(path, bytes);
}

export async function pickSaveLocation(defaultName: string) {
  const api = await getPlatformAPI();
  if (!api.fileOps.pickSaveLocation) {
    throw new Error('pickSaveLocation not supported on this platform');
  }
  return api.fileOps.pickSaveLocation(defaultName);
}

export async function getStorageType() {
  const api = await getPlatformAPI();
  return api.storageType;
}
