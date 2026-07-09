import * as api from "@/api";
import type { SpreadsheetFormatOptions } from "@/types";
import { createAsyncCache } from "@/utils/asyncCache";

const formatOptionsCache = createAsyncCache(api.getSpreadsheetFormatOptions);

export async function spreadsheetFormatOptions(): Promise<SpreadsheetFormatOptions> {
  return formatOptionsCache.get();
}

export async function defaultSpreadsheetExtension(): Promise<string> {
  return (await spreadsheetFormatOptions()).defaultExtension;
}

export async function supportedSpreadsheetExtensions(): Promise<string[]> {
  return [...(await spreadsheetFormatOptions()).supportedExtensions];
}

export async function spreadsheetDialogFilters() {
  return [{ name: "Spreadsheet", extensions: await supportedSpreadsheetExtensions() }];
}
