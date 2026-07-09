import { invoke } from "@tauri-apps/api/core";
import type { OpenFileSelection, PlatformAPI } from '../types';
import type { EditorCommandContext, OpenDocumentResponse, SavedDocumentResponse } from "@/types";

interface PickFileIOSResult extends OpenDocumentResponse {
  info: {
    path: string;
    originalPath: string;
    fileName: string;
  };
}

export const iosFileOps = {
  /** iOS: 后端用官方 dialog/fs 导入到 App 沙盒，不解析、不替换后端活动文档 */
  pickOpenFile: async (): Promise<OpenFileSelection | null> => {
    const info = await invoke<PickFileIOSResult["info"] | null>("pick_open_file_ios");
    if (!info) return null;

    return {
      path: info.path,
      fileName: info.fileName,
      originalPath: info.originalPath,
    };
  },

  discardOpenFileSelection: (selection: OpenFileSelection): Promise<void> => {
    return invoke<void>("discard_open_file_selection_ios", { path: selection.path });
  },

  /** iOS: 从 App 沙盒路径读取并解析（用于最近文件列表） */
  readFile: (path: string): Promise<OpenDocumentResponse> => {
    return invoke<OpenDocumentResponse>("read_file_ios", { path });
  },

  /** iOS: 生成文件字节并写入 App 沙盒路径 */
  saveFile: (path: string, context: EditorCommandContext) =>
    invoke<SavedDocumentResponse>("save_file_ios", { path, ...context }),

  pickSaveLocation: (defaultName: string) =>
    invoke<string | null>("pick_save_location_ios", { defaultName }),

  discardSaveLocation: (path: string): Promise<void> => {
    return invoke<void>("discard_save_location_ios", { path });
  },

  exportFile: (defaultName: string, context: EditorCommandContext) =>
    invoke<string | null>("export_file_ios", { defaultName, ...context }),
};

export const iosAPI: PlatformAPI = {
  fileOps: iosFileOps,
  storageType: 'mobileSandboxPath',
};
