export type RecentFile = {
  id: string;
  path: string;
  fileName: string;
  lastOpened: number;
  fileSize: number;
  thumbnail?: string;
  storageType: 'mobileSandboxPath' | 'desktopPath';
  originalPath?: string;
};
