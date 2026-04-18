import { open, save } from "@tauri-apps/plugin-dialog";
import { readFile, writeFile } from "@tauri-apps/plugin-fs";
import { basename } from "@tauri-apps/api/path";
import type { PlatformAPI } from '../types';

export const desktopFileOps = {
  pickFile: async () => {
    const selected = await open({
      multiple: false,
      filters: [{ name: "Spreadsheet", extensions: ["xlsx", "xls", "csv", "ods"] }],
    });
    if (!selected) return null;
    const bytes = await readFile(selected);
    const bytesArray = Array.from(bytes);
    const fileName = decodeURIComponent(await basename(selected));
    return { path: selected, fileName, bytes: bytesArray };
  },

  readFile: async (path: string) => {
    const bytes = await readFile(path);
    return Array.from(bytes);
  },

  saveFile: async (path: string, bytes: number[]) => {
    await writeFile(path, new Uint8Array(bytes));
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
