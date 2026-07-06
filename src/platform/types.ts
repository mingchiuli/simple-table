/**
 * Platform-agnostic type definitions for dynamic platform loading
 */

import type { OpenDocumentResponse, SavedDocumentResponse } from "@/types";

export interface OpenFileResult extends OpenDocumentResponse {
  path: string;
  fileName: string;
  /** 原始选择来源路径（用于显示/诊断） */
  originalPath?: string;
}

export interface PlatformFileOps {
  /** 打开文件：选择器 + 读取 + 解析（一体化） */
  openFile(): Promise<OpenFileResult | null>;
  /** 从已知路径读取并解析（用于最近文件列表） */
  readFile(path: string): Promise<OpenDocumentResponse>;
  /** 保存文件：生成字节 + 写入（一体化） */
  saveFile(path: string): Promise<SavedDocumentResponse>;
  /** 选择保存位置 */
  pickSaveLocation?(defaultName: string): Promise<string | null>;
  /** iOS: 在 App 沙盒创建新文件 */
  createPrivateFile?(fileName: string): Promise<{ path: string; originalPath: string; fileName: string }>;
  /** iOS: 导出文件 */
  exportFile?(sourcePath: string, defaultName: string): Promise<string | null>;
}

export type StorageType = 'mobileSandboxPath' | 'desktopPath';

export interface PlatformAPI {
  fileOps: PlatformFileOps;
  storageType: StorageType;
}
