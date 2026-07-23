import { invokeCommand } from '@/tauriInvoke';
import type { OpenFileSelection, PlatformAPI } from '../types';
import type {
  EditorCommandContext,
} from '@/types/documentRuntime';
import type { OpenTargetClaim, PreparedOpenDocument } from '@/types/fileRuntime';
import type { RecentFile } from '@/types/recentFileRuntime';
import { runtimePreparedOpenDocument } from '@/application/fileProtocol';

export const desktopFileOps = {
  claimPendingOpenTarget: (): Promise<OpenTargetClaim | null> => {
    return invokeCommand("claim_pending_open_target_desktop", {});
  },

  acknowledgeOpenTarget: (claimId: string): Promise<void> => {
    return invokeCommand("acknowledge_open_target_desktop", { claimId });
  },

  releaseOpenTarget: (claimId: string): Promise<void> => {
    return invokeCommand("release_open_target_desktop", { claimId });
  },

  /** Desktop: 后端选择文件路径并授权随后读取，不在前端直接使用 dialog/fs 插件。 */
  pickOpenFile: async (): Promise<OpenFileSelection | null> => {
    return invokeCommand("pick_open_file_desktop", {});
  },

  discardOpenFileSelection: (selection: OpenFileSelection): Promise<void> => {
    return invokeCommand("discard_open_file_selection_desktop", { path: selection.path });
  },

  /** Desktop: 从后端已授权路径读取并解析。 */
  prepareOpenFile: async (path: string): Promise<PreparedOpenDocument> => {
    return runtimePreparedOpenDocument(await invokeCommand("prepare_open_file_desktop", { path }));
  },

  prepareRecentFile: async (file: RecentFile): Promise<PreparedOpenDocument> => {
    return runtimePreparedOpenDocument(
      await invokeCommand("prepare_recent_file_desktop", { id: file.id }),
    );
  },

  /** Desktop: 生成文件字节并写入路径 */
  saveFile: async (path: string, context: EditorCommandContext, operationId: string) => {
    return invokeCommand("save_file_desktop", { path, ...context, operationId });
  },

  pickSaveLocation: async (defaultName: string) => {
    return invokeCommand("pick_save_location_desktop", { defaultName });
  },

  discardSaveLocation: (path: string): Promise<void> => {
    return invokeCommand("discard_save_location_desktop", { path });
  },

  exportFile: async (defaultName: string, context: EditorCommandContext) => {
    return invokeCommand("export_file_desktop", { defaultName, ...context });
  },
};

export const desktopAPI: PlatformAPI = {
  fileOps: desktopFileOps,
};
