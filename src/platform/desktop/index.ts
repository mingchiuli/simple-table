import { open, save } from "@tauri-apps/plugin-dialog";
import { basename } from "@tauri-apps/api/path";
import { invoke } from "@tauri-apps/api/core";
import type { PlatformAPI, OpenFileResult } from '../types';
import type { OpenDocumentResponse, SavedDocumentResponse } from "@/types";
import { decodeFileNameSegment } from "@/utils/fileFormats";
import { spreadsheetDialogFilters } from "@/utils/spreadsheetFormats";

export const desktopFileOps = {
  /** Desktop: 打开文件选择器 + 直接调用 Rust 解析 */
  openFile: async (): Promise<OpenFileResult | null> => {
    const selected = await open({
      multiple: false,
      filters: await spreadsheetDialogFilters(),
    });
    if (!selected) return null;

    const fileName = decodeFileNameSegment(await basename(selected));

    // 直接调用 Rust 解析（一次调用）
    const document = await invoke<OpenDocumentResponse>("read_file_desktop", { path: selected });

    return {
      ...document,
      path: selected,
      fileName,
    };
  },

  /** Desktop: 从已知路径读取并解析（用于最近文件列表） */
  readFile: async (path: string): Promise<OpenDocumentResponse> => {
    return invoke<OpenDocumentResponse>("read_file_desktop", { path });
  },

  /** Desktop: 生成文件字节并写入路径 */
  saveFile: async (path: string) => {
    return invoke<SavedDocumentResponse>("save_file_desktop", { path });
  },

  pickSaveLocation: async (defaultName: string) => {
    const selected = await save({
      defaultPath: defaultName,
      filters: await spreadsheetDialogFilters(),
    });
    return selected;
  },

  exportFile: async (defaultName: string) => {
    const selected = await save({
      defaultPath: defaultName,
      filters: await spreadsheetDialogFilters(),
    });
    if (!selected) return null;

    await invoke<void>("export_file_desktop", { path: selected });
    return selected;
  },
};

export const desktopAPI: PlatformAPI = {
  fileOps: desktopFileOps,
  storageType: 'desktopPath',
};
