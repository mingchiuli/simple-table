import { invoke } from '@tauri-apps/api/core';
import type { TauriCommandMap } from '@/types/protocol';
import { assertU64String } from '@/utils/u64';

export function invokeCommand<K extends keyof TauriCommandMap>(
  command: K,
  args: TauriCommandMap[K]['args']
): Promise<TauriCommandMap[K]['result']> {
  validateCommandU64Arguments(args);
  return invoke<TauriCommandMap[K]['result']>(command, args);
}

function validateCommandU64Arguments(args: unknown) {
  if (!args || typeof args !== 'object') return;
  const values = args as Record<string, unknown>;
  for (const field of ['documentId', 'baseRevision', 'expectedDocumentId', 'expectedRevision']) {
    const value = values[field];
    if (value !== undefined && value !== null) assertU64String(value, field);
  }
  const request = values.request;
  if (request && typeof request === 'object') {
    validateCommandU64Arguments(request);
  }
}
