/**
 * Platform loader - Dynamic import factory with caching
 */
import { getPlatform } from '@/utils/platform';
import type { PlatformAPI, OpenFileResult } from './types';
import type { FileData } from '@/types';

let cachedAPI: PlatformAPI | null = null;
let loadingPromise: Promise<PlatformAPI> | null = null;

async function loadPlatformAPI(): Promise<PlatformAPI> {
  const platform = getPlatform();

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

// ==================== Convenience re-exports ====================

/** 打开文件：选择器 + 读取 + 解析（一体化） */
export async function openFile(): Promise<OpenFileResult | null> {
  const api = await getPlatformAPI();
  return api.fileOps.openFile();
}

/** 从已知路径读取并解析（用于最近文件列表） */
export async function readFile(path: string): Promise<FileData> {
  const api = await getPlatformAPI();
  return api.fileOps.readFile(path);
}

/** 保存文件：生成字节 + 写入（一体化） */
export async function saveFile(path: string, fileData: FileData) {
  const api = await getPlatformAPI();
  return api.fileOps.saveFile(path, fileData);
}

/** 选择保存位置 */
export async function pickSaveLocation(defaultName: string) {
  const api = await getPlatformAPI();
  if (!api.fileOps.pickSaveLocation) {
    throw new Error('pickSaveLocation not supported on this platform');
  }
  return api.fileOps.pickSaveLocation(defaultName);
}

/** 导出沙盒文件到用户选择的位置 */
export async function exportFile(sourcePath: string, defaultName: string) {
  const api = await getPlatformAPI();
  if (!api.fileOps.exportFile) {
    throw new Error('exportFile not supported on this platform');
  }
  return api.fileOps.exportFile(sourcePath, defaultName);
}

/** 获取存储类型 */
export async function getStorageType() {
  const api = await getPlatformAPI();
  return api.storageType;
}
