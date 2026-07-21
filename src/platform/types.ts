/**
 * Platform-agnostic type definitions for dynamic platform loading
 */

import type {
  EditorCommandContext,
} from '@/types/documentRuntime';
import type { PreparedOpenDocument } from '@/types/fileRuntime';
import type { RecentFile } from '@/types/recentFileRuntime';
import type { SavedDocumentResponse } from '@/types/protocol';

export type OpenFileSelection = {
  path: string;
  fileName: string;
  /** 原始选择来源路径（用于显示/诊断） */
  originalPath?: string;
};

export interface PlatformFileOps {
  /** 只选择/导入文件，不解析、不替换后端活动文档 */
  pickOpenFile(): Promise<OpenFileSelection | null>;
  /** 释放已选择/导入但没有被当前文档接管的文件；桌面路径通常无需实现 */
  discardOpenFileSelection?(selection: OpenFileSelection): Promise<void>;
  /** 从已知路径读取并解析（用于最近文件列表） */
  prepareOpenFile(path: string): Promise<PreparedOpenDocument>;
  /** 从平台受信任的最近文件记录读取并解析；未实现的平台回退到 prepareOpenFile(file.path) */
  prepareRecentFile?(file: RecentFile): Promise<PreparedOpenDocument>;
  /** 保存文件：生成字节 + 写入（一体化） */
  saveFile(path: string, context: EditorCommandContext): Promise<SavedDocumentResponse>;
  /** 选择保存位置 */
  pickSaveLocation?(defaultName: string): Promise<string | null>;
  /** 释放已预留但没有成功保存接管的保存目标；桌面路径通常无需实现 */
  discardSaveLocation?(path: string): Promise<void>;
  /** 导出当前编辑状态到用户选择的位置 */
  exportFile?(defaultName: string, context: EditorCommandContext): Promise<string | null>;
}

export interface PlatformAPI {
  fileOps: PlatformFileOps;
}
