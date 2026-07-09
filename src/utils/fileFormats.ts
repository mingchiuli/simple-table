export type SpreadsheetFileExtension = "xlsx" | "csv";

export const DEFAULT_SPREADSHEET_EXTENSION: SpreadsheetFileExtension = "xlsx";
export const SUPPORTED_SPREADSHEET_EXTENSIONS: SpreadsheetFileExtension[] = ["xlsx", "csv"];

export function extensionFromName(name: string): string | null {
  const fileName = name.split(/[\\/]/).pop() ?? name;
  const dotIndex = fileName.lastIndexOf(".");
  if (dotIndex <= 0 || dotIndex === fileName.length - 1) {
    return null;
  }
  return fileName.slice(dotIndex + 1).toLowerCase();
}

export function supportedSpreadsheetExtension(
  name: string
): SpreadsheetFileExtension | null {
  const extension = extensionFromName(name);
  return SUPPORTED_SPREADSHEET_EXTENSIONS.includes(extension as SpreadsheetFileExtension)
    ? (extension as SpreadsheetFileExtension)
    : null;
}

export function baseNameWithoutExtension(name: string): string {
  const fileName = name.split(/[\\/]/).pop() ?? name;
  const dotIndex = fileName.lastIndexOf(".");
  if (dotIndex <= 0) {
    return fileName || "untitled";
  }
  return fileName.slice(0, dotIndex) || "untitled";
}

export function isUntitledSpreadsheet(name: string): boolean {
  return baseNameWithoutExtension(name).startsWith("untitled");
}
