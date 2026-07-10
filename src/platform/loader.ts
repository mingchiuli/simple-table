/**
 * Platform loader - Dynamic import factory with caching
 */
import { getPlatform } from '@/utils/platform';
import type { PlatformAPI, OpenFileSelection } from './types';
import type { EditorCommandContext, OpenDocumentResponse, RecentFile } from '@/types';
import { createAsyncCache } from '@/utils/asyncCache';

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

/** 只选择/导入文件，不解析、不替换后端活动文档 */
export async function pickOpenFile(): Promise<OpenFileSelection | null> {
  const api = await getPlatformAPI();
  return api.fileOps.pickOpenFile();
}

/** 丢弃已选择/导入但没有成功打开为当前文档的文件 */
export async function discardOpenFileSelection(selection: OpenFileSelection): Promise<void> {
  const api = await getPlatformAPI();
  await api.fileOps.discardOpenFileSelection?.(selection);
}

/** 从已知路径读取并解析（用于最近文件列表） */
export async function readFile(path: string): Promise<OpenDocumentResponse> {
  const api = await getPlatformAPI();
  return api.fileOps.readFile(path);
}

/** 从平台受信任的最近文件记录读取并解析 */
export async function readRecentFile(file: RecentFile): Promise<OpenDocumentResponse> {
  const api = await getPlatformAPI();
  return api.fileOps.readRecentFile?.(file) ?? api.fileOps.readFile(file.path);
}

/** 保存文件：生成字节 + 写入（一体化） */
export async function saveFile(path: string, context: EditorCommandContext) {
  const api = await getPlatformAPI();
  return api.fileOps.saveFile(path, context);
}

/** 选择保存位置 */
export async function pickSaveLocation(defaultName: string) {
  const api = await getPlatformAPI();
  if (!api.fileOps.pickSaveLocation) {
    throw new Error('pickSaveLocation not supported on this platform');
  }
  return api.fileOps.pickSaveLocation(defaultName);
}

/** 丢弃已预留但没有成功保存接管的保存目标 */
export async function discardSaveLocation(path: string): Promise<void> {
  const api = await getPlatformAPI();
  await api.fileOps.discardSaveLocation?.(path);
}

/** 导出当前编辑状态到用户选择的位置 */
export async function exportFile(defaultName: string, context: EditorCommandContext) {
  const api = await getPlatformAPI();
  if (!api.fileOps.exportFile) {
    throw new Error('exportFile not supported on this platform');
  }
  return api.fileOps.exportFile(defaultName, context);
}
