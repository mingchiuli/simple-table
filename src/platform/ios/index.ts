import { invoke } from "@tauri-apps/api/core";
import type { PlatformAPI, OpenFileResult } from '../types';
import type { EditorCommandContext, OpenDocumentResponse, SavedDocumentResponse } from "@/types";

interface PickFileIOSResult extends OpenDocumentResponse {
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
      editorSession: result.editorSession,
      path: result.info.path,
      fileName: result.info.fileName,
      originalPath: result.info.originalPath,
    };
  },

  /** iOS: 从 App 沙盒路径读取并解析（用于最近文件列表） */
  readFile: (path: string): Promise<OpenDocumentResponse> => {
    return invoke<OpenDocumentResponse>("read_file_ios", { path });
  },

  /** iOS: 生成文件字节并写入 App 沙盒路径 */
  saveFile: (path: string, context: EditorCommandContext) =>
    invoke<SavedDocumentResponse>("save_file_ios", { path, ...context }),

  createPrivateFile: (fileName: string) =>
    invoke<{ path: string; originalPath: string; fileName: string }>("create_private_file_ios", { fileName }),

  pickSaveLocation: async (defaultName: string) => {
    const info = await invoke<{ path: string; originalPath: string; fileName: string }>("create_private_file_ios", { fileName: defaultName });
    return info.path;
  },

  exportFile: (defaultName: string, context: EditorCommandContext) =>
    invoke<string | null>("export_file_ios", { defaultName, ...context }),
};

export const iosAPI: PlatformAPI = {
  fileOps: iosFileOps,
  storageType: 'mobileSandboxPath',
};
