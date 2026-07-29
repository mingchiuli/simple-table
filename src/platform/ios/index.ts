import { invokeCommand } from '@/tauriInvoke';
import type { OpenFileSelection, PlatformAPI } from '../types';
import type { EditorCommandContext } from '@/types/documentRuntime';
import type { PreparedOpenDocument } from '@/types/fileRuntime';
import {
  runtimeFileOperationReceipt,
  runtimePreparedOpenDocument,
} from '@/application/fileProtocol';

export const iosFileOps = {
  /** iOS: 后端用官方 dialog/fs 导入到 App 沙盒，不解析、不替换后端活动文档 */
  pickOpenFile: async (): Promise<OpenFileSelection | null> => {
    const info = await invokeCommand("pick_open_file_ios", {});
    if (!info) return null;

    return {
      path: info.path,
      fileName: info.fileName,
      originalPath: info.originalPath,
    };
  },

  discardOpenFileSelection: (selection: OpenFileSelection): Promise<void> => {
    return invokeCommand("discard_open_file_selection_ios", { path: selection.path });
  },

  /** iOS: 从 App 沙盒路径读取并解析（用于最近文件列表） */
  prepareOpenFile: async (path: string, preparationId: string): Promise<PreparedOpenDocument> => {
    return runtimePreparedOpenDocument(
      await invokeCommand("prepare_open_file_ios", { path, preparationId }),
    );
  },

  /** iOS: 生成文件字节并写入 App 沙盒路径 */
  saveFile: (path: string, context: EditorCommandContext, operationId: string) =>
    invokeCommand("save_file_ios", { path, ...context, operationId }),

  pickSaveLocation: (defaultName: string) =>
    invokeCommand("pick_save_location_ios", { defaultName }),

  discardSaveLocation: (path: string): Promise<void> => {
    return invokeCommand("discard_save_location_ios", { path });
  },

  exportFile: async (
    defaultName: string,
    context: EditorCommandContext,
    operationId: string,
  ) => {
    const receipt = await invokeCommand("export_file_ios", {
      defaultName,
      ...context,
      operationId,
    });
    return receipt ? runtimeFileOperationReceipt(receipt) : null;
  },
};

export const iosAPI: PlatformAPI = {
  fileOps: iosFileOps,
};
