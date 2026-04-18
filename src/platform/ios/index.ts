import { invoke } from "@tauri-apps/api/core";
import { readFile } from "@tauri-apps/plugin-fs";
import type { PlatformAPI, PickedFileWithOrigin } from '../types';

export const iosFileOps = {
  pickFile: async () => {
    const result = await invoke<PickedFileWithOrigin | null>("pick_file_ios");
    if (!result) return null;
    return { path: result.path, originalPath: result.originalPath, fileName: result.fileName, bytes: [] };
  },

  readFile: async (path: string) => {
    const bytes = await readFile(path);
    return Array.from(bytes);
  },

  saveFile: (path: string, bytes: number[]) =>
    invoke<void>("save_file_ios", { path, bytes }),

  createPrivateFile: (fileName: string) =>
    invoke<PickedFileWithOrigin>("create_private_file_ios", { fileName }),

  exportFile: (sourcePath: string, defaultName: string) =>
    invoke<string | null>("export_file_ios", { sourcePath, defaultName }),

  silentExport: (sourcePath: string, destPath: string) =>
    invoke<void>("silent_export_file_ios", { sourcePath, destPath }),
};

export const iosAPI: PlatformAPI = {
  fileOps: iosFileOps,
  storageType: 'iosPrivate',
};
