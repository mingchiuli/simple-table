import { invoke } from "@tauri-apps/api/core";
import type { OpenFileSelection, PlatformAPI } from '../types';
import type { EditorCommandContext, OpenDocumentResponse, SavedDocumentResponse } from "@/types";

interface PickFileAndroidResult extends OpenDocumentResponse {
  info: {
    path: string;
    originalPath: string;
    fileName: string;
  };
}

export const androidFileOps = {
  /** Android: 后端用官方 dialog/fs 导入到 App 沙盒，不解析、不替换后端活动文档 */
  pickOpenFile: async (): Promise<OpenFileSelection | null> => {
    const info = await invoke<PickFileAndroidResult["info"] | null>("pick_open_file_android");
    if (!info) return null;

    return {
      path: info.path,
      fileName: info.fileName,
      originalPath: info.originalPath,
    };
  },

  /** Android: 从 App 沙盒路径读取并解析（用于最近文件列表） */
  readFile: (path: string): Promise<OpenDocumentResponse> => {
    return invoke<OpenDocumentResponse>("read_file_android", { path });
  },

  /** Android: 生成文件字节并写入 App 沙盒路径 */
  saveFile: (path: string, context: EditorCommandContext) =>
    invoke<SavedDocumentResponse>("save_file_android", { path, ...context }),

  exportFile: (defaultName: string, context: EditorCommandContext) =>
    invoke<string | null>("export_file_android", { defaultName, ...context }),

  pickSaveLocation: (defaultName: string) =>
    invoke<string | null>("pick_save_location_android", { defaultName }),
};

export const androidAPI: PlatformAPI = {
  fileOps: androidFileOps,
  storageType: 'mobileSandboxPath',
};
