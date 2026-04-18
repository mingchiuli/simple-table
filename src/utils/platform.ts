import { platform } from '@tauri-apps/plugin-os';

let cachedPlatform: string | null = null;

/**
 * 获取当前平台类型（带缓存）
 */
export function getPlatform(): string {
  if (cachedPlatform) {
    return cachedPlatform;
  }
  cachedPlatform = platform();
  return cachedPlatform;
}

/**
 * 是否为 Android 平台
 */
export function isAndroid(): boolean {
  return getPlatform() === 'android';
}

/**
 * 是否为 iOS 平台
 */
export function isIOS(): boolean {
  return getPlatform() === 'ios';
}

/**
 * 是否为移动端（Android 或 iOS）
 */
export function isMobile(): boolean {
  const p = getPlatform();
  return p === 'android' || p === 'ios';
}

/**
 * 是否为桌面端（macOS, Windows, Linux）
 */
export function isDesktop(): boolean {
  const p = getPlatform();
  return p === 'macos' || p === 'windows' || p === 'linux';
}