import type { RecentFile as ProtocolRecentFile } from '@/types/protocol';
import type { RecentFile } from '@/types/recentFileRuntime';

export function runtimeRecentFile(file: ProtocolRecentFile): RecentFile {
  return {
    id: file.id,
    path: file.path,
    fileName: file.fileName,
    lastOpened: file.lastOpened,
    fileSize: file.fileSize,
    thumbnail: file.thumbnail,
    storageType: file.storageType,
    originalPath: file.originalPath,
  };
}
