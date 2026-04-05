import { platform } from '@tauri-apps/plugin-os';

let cachedPlatform: string | null = null;

/**
 * 获取当前平台类型（带缓存）
 */
export async function getPlatform(): Promise<string> {
  if (cachedPlatform) {
    return cachedPlatform;
  }
  cachedPlatform = platform();
  return cachedPlatform;
}

/**
 * 是否为 Android 平台
 */
export async function isAndroid(): Promise<boolean> {
  return await getPlatform() === 'android';
}

/**
 * 是否为 iOS 平台
 */
export async function isIOS(): Promise<boolean> {
  return await getPlatform() === 'ios';
}

/**
 * 是否为移动端（Android 或 iOS）
 */
export async function isMobile(): Promise<boolean> {
  const p = await getPlatform();
  return p === 'android' || p === 'ios';
}

/**
 * 是否为桌面端（macOS, Windows, Linux）
 */
export async function isDesktop(): Promise<boolean> {
  const p = await getPlatform();
  return p === 'macos' || p === 'windows' || p === 'linux';
}