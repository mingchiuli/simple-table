export type NativeSaveExtension = 'xlsx';
export type ExportExtension = NativeSaveExtension | 'csv';

export type DocumentCapabilities = {
  nativeSaveExtension: NativeSaveExtension | null;
  exportExtension: ExportExtension;
  requiresSaveAsForNativeSave: boolean;
};

export function nativeSaveExtension(fileName: string): NativeSaveExtension | null {
  const extension = extensionOf(fileName) || 'xlsx';
  return extension === 'xlsx' ? extension : null;
}

export function exportExtension(fileName: string): ExportExtension | null {
  const extension = extensionOf(fileName) || 'xlsx';
  return extension === 'xlsx' || extension === 'csv' ? extension : null;
}

export function documentCapabilities(fileName: string, currentPath: string | null): DocumentCapabilities {
  const sourceName = currentPath || fileName;
  const nativeExtension = nativeSaveExtension(sourceName);
  const exportExt = exportExtension(fileName) ?? 'xlsx';

  return {
    nativeSaveExtension: nativeExtension,
    exportExtension: exportExt,
    requiresSaveAsForNativeSave: !nativeExtension,
  };
}

function extensionOf(fileName: string): string | null {
  return fileName.split('.').pop()?.toLowerCase() || null;
}
