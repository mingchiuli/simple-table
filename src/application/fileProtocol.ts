import { runtimeWorkbookCapabilities } from '@/application/editorRuntimeProtocol';
import type {
  DocumentCapabilities as ProtocolDocumentCapabilities,
  NativeSavePlan as ProtocolNativeSavePlan,
  SpreadsheetFormatOptions as ProtocolSpreadsheetFormatOptions,
} from '@/types/protocol';
import type {
  DocumentCapabilities,
  NativeSavePlan,
  SpreadsheetFormatOptions,
} from '@/types/fileRuntime';

export function runtimeDocumentCapabilities(
  capabilities: ProtocolDocumentCapabilities,
): DocumentCapabilities {
  return {
    sourceFormat: capabilities.sourceFormat,
    canSaveOriginal: capabilities.canSaveOriginal,
    nativeSaveFormat: capabilities.nativeSaveFormat,
    exportFormats: [...capabilities.exportFormats],
    nativeSaveExtension: capabilities.nativeSaveExtension,
    exportExtension: capabilities.exportExtension,
    requiresSaveAsForNativeSave: capabilities.requiresSaveAsForNativeSave,
    workbook: runtimeWorkbookCapabilities(capabilities.workbook),
  };
}

export function runtimeNativeSavePlan(plan: ProtocolNativeSavePlan): NativeSavePlan {
  return {
    canSave: plan.canSave,
    requiresSaveAs: plan.requiresSaveAs,
    nativeSaveExtension: plan.nativeSaveExtension,
    defaultExtension: plan.defaultExtension,
    blockedReason: plan.blockedReason,
    capabilities: runtimeDocumentCapabilities(plan.capabilities),
  };
}

export function runtimeSpreadsheetFormatOptions(
  options: ProtocolSpreadsheetFormatOptions,
): SpreadsheetFormatOptions {
  return {
    defaultExtension: options.defaultExtension,
    supportedExtensions: [...options.supportedExtensions],
  };
}
