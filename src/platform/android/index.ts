import { invokeCommand } from '@/tauriInvoke';
import type { OpenFileSelection, PlatformAPI } from '../types';
import type { EditorCommandContext } from '@/types/documentRuntime';
import type { PreparedOpenDocument } from '@/types/fileRuntime';
import { runtimePreparedOpenDocument } from '@/application/fileProtocol';

export const androidFileOps = {
  /** Android: 后端用官方 dialog/fs 导入到 App 沙盒，不解析、不替换后端活动文档 */
  pickOpenFile: async (): Promise<OpenFileSelection | null> => {
    const info = await invokeCommand("pick_open_file_android", {});
    if (!info) return null;

    return {
      path: info.path,
      fileName: info.fileName,
      originalPath: info.originalPath,
    };
  },

  discardOpenFileSelection: (selection: OpenFileSelection): Promise<void> => {
    return invokeCommand("discard_open_file_selection_android", { path: selection.path });
  },

  /** Android: 从 App 沙盒路径读取并解析（用于最近文件列表） */
  prepareOpenFile: async (path: string): Promise<PreparedOpenDocument> => {
    return runtimePreparedOpenDocument(await invokeCommand("prepare_open_file_android", { path }));
  },

  /** Android: 生成文件字节并写入 App 沙盒路径 */
  saveFile: (path: string, context: EditorCommandContext, operationId: string) =>
    invokeCommand("save_file_android", { path, ...context, operationId }),

  exportFile: (defaultName: string, context: EditorCommandContext) =>
    invokeCommand("export_file_android", { defaultName, ...context }),

  pickSaveLocation: (defaultName: string) =>
    invokeCommand("pick_save_location_android", { defaultName }),

  discardSaveLocation: (path: string): Promise<void> => {
    return invokeCommand("discard_save_location_android", { path });
  },
};

export const androidAPI: PlatformAPI = {
  fileOps: androidFileOps,
};
