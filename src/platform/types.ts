/**
 * Platform-agnostic type definitions for dynamic platform loading
 */

export interface PickedFile {
  path: string;
  fileName: string;
  bytes: number[];
}

export interface PickedFileWithOrigin {
  path: string;
  originalPath: string;
  fileName: string;
  bytes?: number[];
}

export interface PlatformFileOps {
  pickFile(): Promise<PickedFile | PickedFileWithOrigin | null>;
  readFile(path: string): Promise<number[]>;
  saveFile(path: string, bytes: number[]): Promise<void>;
  pickSaveLocation?(defaultName: string): Promise<string | null>;
  createPrivateFile?(fileName: string): Promise<PickedFileWithOrigin>;
  exportFile?(sourcePath: string, defaultName: string): Promise<string | null>;
  silentExport?(sourcePath: string, destPath: string): Promise<void>;
}

export type StorageType = 'androidUri' | 'iosPrivate' | 'desktopPath';

export interface PlatformAPI {
  fileOps: PlatformFileOps;
  storageType: StorageType;
}
