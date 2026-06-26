import { invoke } from "@tauri-apps/api/core";
import type { PlatformAPI, OpenFileResult } from '../types';
import type { FileData } from "@/types";

interface PickFileIOSResult {
  fileData: FileData;
  info: {
    path: string;
    originalPath: string;
    fileName: string;
  };
}

export const iosFileOps = {
  /** iOS: 后端用官方 dialog/fs 导入到 App 沙盒并解析 */
  openFile: async (): Promise<OpenFileResult | null> => {
    const result = await invoke<PickFileIOSResult | null>("pick_file_ios");
    if (!result) return null;

    return {
      fileData: result.fileData,
      path: result.info.path,
      fileName: result.info.fileName,
      originalPath: result.info.originalPath,
    };
  },

  /** iOS: 从 App 沙盒路径读取并解析（用于最近文件列表） */
  readFile: (path: string): Promise<FileData> => {
    return invoke<FileData>("read_file_ios", { path });
  },

  /** iOS: 生成文件字节并写入 App 沙盒路径 */
  saveFile: (path: string) =>
    invoke<void>("save_file_ios", { path }),

  createPrivateFile: (fileName: string) =>
    invoke<{ path: string; originalPath: string; fileName: string }>("create_private_file_ios", { fileName }),

  pickSaveLocation: async (defaultName: string) => {
    const info = await invoke<{ path: string; originalPath: string; fileName: string }>("create_private_file_ios", { fileName: defaultName });
    return info.path;
  },

  exportFile: (sourcePath: string, defaultName: string) =>
    invoke<string | null>("export_file_ios", { sourcePath, defaultName }),
};

export const iosAPI: PlatformAPI = {
  fileOps: iosFileOps,
  storageType: 'mobileSandboxPath',
};
