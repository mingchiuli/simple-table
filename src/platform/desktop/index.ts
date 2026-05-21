import { open, save } from "@tauri-apps/plugin-dialog";
import { readFile } from "@tauri-apps/plugin-fs";
import { basename } from "@tauri-apps/api/path";
import { invoke } from "@tauri-apps/api/core";
import type { PlatformAPI, OpenFileResult } from '../types';
import type { FileData } from "@/types";

export const desktopFileOps = {
  /** Desktop: 打开文件选择器 + 直接调用 Rust 解析 */
  openFile: async (): Promise<OpenFileResult | null> => {
    const selected = await open({
      multiple: false,
      filters: [{ name: "Spreadsheet", extensions: ["xlsx", "xls", "csv", "ods"] }],
    });
    if (!selected) return null;

    const fileName = decodeURIComponent(await basename(selected));
    const bytes = await readFile(selected);
    const bytesArray = Array.from(bytes);

    // 直接调用 Rust 解析（一次调用）
    const fileData = await invoke<FileData>("read_file_desktop", { path: selected });

    return {
      fileData,
      path: selected,
      fileName,
      bytes: bytesArray, // 用于缩略图
    };
  },

  /** Desktop: 从已知路径读取并解析（用于最近文件列表） */
  readFile: async (path: string): Promise<FileData> => {
    return invoke<FileData>("read_file_desktop", { path });
  },

  /** Desktop: 生成文件字节并写入路径 */
  saveFile: async (path: string, fileData: FileData) => {
    await invoke<void>("save_file_desktop", { path, fileData });
  },

  pickSaveLocation: async (defaultName: string) => {
    const selected = await save({
      defaultPath: defaultName,
      filters: [{ name: "Spreadsheet", extensions: ["xlsx", "csv"] }],
    });
    return selected;
  },
};

export const desktopAPI: PlatformAPI = {
  fileOps: desktopFileOps,
  storageType: 'desktopPath',
};