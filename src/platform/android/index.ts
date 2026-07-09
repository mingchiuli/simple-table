import { invoke } from "@tauri-apps/api/core";
import type { PlatformAPI, OpenFileResult } from '../types';
import type { OpenDocumentResponse, SavedDocumentResponse } from "@/types";

interface PickFileAndroidResult extends OpenDocumentResponse {
  info: {
    path: string;
    originalPath: string;
    fileName: string;
  };
}

export const androidFileOps = {
  /** Android: 后端用官方 dialog/fs 导入到 App 沙盒并解析 */
  openFile: async (): Promise<OpenFileResult | null> => {
    const result = await invoke<PickFileAndroidResult | null>("pick_file_android");
    if (!result) return null;

    return {
      fileData: result.fileData,
      editorSession: result.editorSession,
      path: result.info.path,
      fileName: result.info.fileName,
      originalPath: result.info.originalPath,
    };
  },

  /** Android: 从 App 沙盒路径读取并解析（用于最近文件列表） */
  readFile: (path: string): Promise<OpenDocumentResponse> => {
    return invoke<OpenDocumentResponse>("read_file_android", { path });
  },

  /** Android: 生成文件字节并写入 App 沙盒路径 */
  saveFile: (path: string) =>
    invoke<SavedDocumentResponse>("save_file_android", { path }),

  exportFile: (defaultName: string) =>
    invoke<string | null>("export_file_android", { defaultName }),

  pickSaveLocation: (defaultName: string) =>
    invoke<string | null>("pick_save_location_android", { defaultName }),
};

export const androidAPI: PlatformAPI = {
  fileOps: androidFileOps,
  storageType: 'mobileSandboxPath',
};
