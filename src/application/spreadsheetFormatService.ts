import type { SpreadsheetFormatOptions } from "@/types";
import { createAsyncCache } from "@/utils/asyncCache";

export type SpreadsheetFormatPort = {
  getSpreadsheetFormatOptions(): Promise<SpreadsheetFormatOptions>;
};

export function createSpreadsheetFormatService(port: SpreadsheetFormatPort) {
  const formatOptionsCache = createAsyncCache(port.getSpreadsheetFormatOptions);

  async function spreadsheetFormatOptions(): Promise<SpreadsheetFormatOptions> {
    return formatOptionsCache.get();
  }

  async function defaultSpreadsheetExtension(): Promise<string> {
    return (await spreadsheetFormatOptions()).defaultExtension;
  }

  async function supportedSpreadsheetExtensions(): Promise<string[]> {
    return [...(await spreadsheetFormatOptions()).supportedExtensions];
  }

  return {
    spreadsheetFormatOptions,
    defaultSpreadsheetExtension,
    supportedSpreadsheetExtensions,
  };
}
