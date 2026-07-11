import { invoke } from '@tauri-apps/api/core';
import type { TauriCommandMap } from '@/types';

export function invokeCommand<K extends keyof TauriCommandMap>(
  command: K,
  args: TauriCommandMap[K]['args']
): Promise<TauriCommandMap[K]['result']> {
  return invoke<TauriCommandMap[K]['result']>(command, args);
}
