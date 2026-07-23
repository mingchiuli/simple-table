import type { RuntimeWorkbookCapabilities } from './editorRuntime';

export type PreparedOpenDocument = {
  token: string;
};

export type OpenTargetClaim = {
  claimId: string;
  path: string;
};

export type NativeSavePlan = {
  canSave: boolean;
  requiresSaveAs: boolean;
  nativeSaveExtension: 'xlsx' | 'csv' | null;
  defaultExtension: 'xlsx' | 'csv';
  blockedReason?: string | null;
  capabilities: DocumentCapabilities;
};

export type DocumentCapabilities = {
  sourceFormat: 'xlsx' | 'csv';
  canSaveOriginal: boolean;
  nativeSaveFormat: 'xlsx' | 'csv' | null;
  exportFormats: Array<'xlsx' | 'csv'>;
  nativeSaveExtension: 'xlsx' | 'csv' | null;
  exportExtension: 'xlsx' | 'csv';
  requiresSaveAsForNativeSave: boolean;
  workbook: RuntimeWorkbookCapabilities;
};

export type SpreadsheetFormatOptions = {
  defaultExtension: string;
  supportedExtensions: string[];
};
