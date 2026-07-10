import { invoke } from "@tauri-apps/api/core";
import type { OpenFileSelection, PlatformAPI } from '../types';
import type {
  EditorCommandContext,
  OpenDocumentResponse,
  RecentFile,
  SavedDocumentResponse,
} from "@/types";

export const desktopFileOps = {
  /** Desktop: 后端选择文件路径并授权随后读取，不在前端直接使用 dialog/fs 插件。 */
  pickOpenFile: async (): Promise<OpenFileSelection | null> => {
    return invoke<OpenFileSelection | null>("pick_open_file_desktop");
  },

  discardOpenFileSelection: (selection: OpenFileSelection): Promise<void> => {
    return invoke<void>("discard_open_file_selection_desktop", { path: selection.path });
  },

  /** Desktop: 从后端已授权路径读取并解析。 */
  readFile: async (path: string): Promise<OpenDocumentResponse> => {
    return invoke<OpenDocumentResponse>("read_file_desktop", { path });
  },

  readRecentFile: (file: RecentFile): Promise<OpenDocumentResponse> => {
    return invoke<OpenDocumentResponse>("read_recent_file_desktop", { id: file.id });
  },

  /** Desktop: 生成文件字节并写入路径 */
  saveFile: async (path: string, context: EditorCommandContext) => {
    return invoke<SavedDocumentResponse>("save_file_desktop", { path, ...context });
  },

  pickSaveLocation: async (defaultName: string) => {
    return invoke<string | null>("pick_save_location_desktop", { defaultName });
  },

  discardSaveLocation: (path: string): Promise<void> => {
    return invoke<void>("discard_save_location_desktop", { path });
  },

  exportFile: async (defaultName: string, context: EditorCommandContext) => {
    return invoke<string | null>("export_file_desktop", { defaultName, ...context });
  },
};

export const desktopAPI: PlatformAPI = {
  fileOps: desktopFileOps,
};
