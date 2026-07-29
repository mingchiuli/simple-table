import type { DocumentSessionStateInput, U64String } from './documentRuntime';
import type { DocumentStatusStateInput, RuntimeWorkbookCapabilities } from './editorRuntime';

export type PreparedOpenDocument = {
  token: string;
  preview: {
    session: DocumentSessionStateInput;
    status: DocumentStatusStateInput;
    manifestResidentBytes: number;
  };
};

export type FileOperationKind = 'open' | 'save' | 'close' | 'export';

export type FileOperationReceipt = {
  kind: FileOperationKind;
  documentId: U64String;
  revision: U64String;
  path: string;
  fileName: string;
};

export type FileOperationResultLookup = {
  status: 'pending' | 'completed' | 'failed' | 'cancelled' | 'missing';
  receipt?: FileOperationReceipt;
  error?: FileOperationFailure;
};

export type FileOperationFailure = {
  code: string;
  message: string;
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
