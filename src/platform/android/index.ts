import { invoke } from "@tauri-apps/api/core";
import type { PlatformAPI, PickedFile } from '../types';

export const androidFileOps = {
  pickFile: () => invoke<PickedFile | null>("pick_file_android"),

  readFile: (uri: string) => invoke<number[]>("read_file_android", { uri }),

  saveFile: (uri: string, bytes: number[]) =>
    invoke<void>("save_file_android", { uri, bytes }),

  pickSaveLocation: (defaultName: string) =>
    invoke<string | null>("pick_save_location_android", { defaultName }),
};

export const androidAPI: PlatformAPI = {
  fileOps: androidFileOps,
  storageType: 'androidUri',
};
