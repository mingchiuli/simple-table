import * as api from "@/api";
import type { DocumentCapabilities } from "@/types";

export type NativeSaveExtension = "xlsx";
export type ExportExtension = NativeSaveExtension | "csv";

export function nativeSaveExtensionFromName(fileName: string): NativeSaveExtension | null {
  const extension = extensionOf(fileName) || 'xlsx';
  return extension === 'xlsx' ? extension : null;
}

export function exportExtensionFromName(fileName: string): ExportExtension | null {
  const extension = extensionOf(fileName) || 'xlsx';
  return extension === 'xlsx' || extension === 'csv' ? extension : null;
}

export async function documentCapabilities(
  fileName: string,
  currentPath: string | null
): Promise<DocumentCapabilities> {
  return api.getDocumentCapabilities(fileName, currentPath);
}

function extensionOf(fileName: string): string | null {
  return fileName.split('.').pop()?.toLowerCase() || null;
}
