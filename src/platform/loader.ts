/**
 * Platform loader - Dynamic import factory with caching
 */
import { getPlatform } from '@/utils/platform';
import { basename } from '@tauri-apps/api/path';
import type { PlatformAPI, OpenFileResult } from './types';
import type { OpenDocumentResponse } from '@/types';
import { createAsyncCache } from '@/utils/asyncCache';
import { decodeFileNameSegment, fileNameFromPathLike } from '@/utils/fileFormats';

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

const platformAPICache = createAsyncCache(loadPlatformAPI);

/**
 * Get the platform API (cached after first load)
 */
export async function getPlatformAPI(): Promise<PlatformAPI> {
  return platformAPICache.get();
}

// ==================== Convenience re-exports ====================

/** 打开文件：选择器 + 读取 + 解析（一体化） */
export async function openFile(): Promise<OpenFileResult | null> {
  const api = await getPlatformAPI();
  return api.fileOps.openFile();
}

/** 从已知路径读取并解析（用于最近文件列表） */
export async function readFile(path: string): Promise<OpenDocumentResponse> {
  const api = await getPlatformAPI();
  return api.fileOps.readFile(path);
}

/** 保存文件：生成字节 + 写入（一体化） */
export async function saveFile(path: string) {
  const api = await getPlatformAPI();
  return api.fileOps.saveFile(path);
}

/** 选择保存位置 */
export async function pickSaveLocation(defaultName: string) {
  const api = await getPlatformAPI();
  if (!api.fileOps.pickSaveLocation) {
    throw new Error('pickSaveLocation not supported on this platform');
  }
  return api.fileOps.pickSaveLocation(defaultName);
}

/** 导出当前编辑状态到用户选择的位置 */
export async function exportFile(defaultName: string) {
  const api = await getPlatformAPI();
  if (!api.fileOps.exportFile) {
    throw new Error('exportFile not supported on this platform');
  }
  return api.fileOps.exportFile(defaultName);
}

/** 获取存储类型 */
export async function getStorageType() {
  const api = await getPlatformAPI();
  return api.storageType;
}

/** 获取路径中的文件名 */
export async function getFileName(path: string) {
  try {
    return decodeFileNameSegment(await basename(path));
  } catch {
    return fileNameFromPathLike(path, "unknown");
  }
}
