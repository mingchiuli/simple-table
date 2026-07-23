import {
  editorSessionStatusState,
  runtimeWorkbookCapabilities,
} from '@/application/editorRuntimeProtocol';
import { openSessionState } from '@/application/documentSessionProtocol';
import { admitDocumentManifestResidentBytes } from '@/projection/documentProjection';
import type {
  DocumentCapabilities as ProtocolDocumentCapabilities,
  NativeSavePlan as ProtocolNativeSavePlan,
  SpreadsheetFormatOptions as ProtocolSpreadsheetFormatOptions,
  PreparedOpenDocument as ProtocolPreparedOpenDocument,
  FileOperationReceipt as ProtocolFileOperationReceipt,
  FileOperationResultLookup as ProtocolFileOperationResultLookup,
  OpenDocumentResponse,
  SavedDocumentResponse,
} from '@/types/protocol';
import type {
  DocumentCapabilities,
  NativeSavePlan,
  SpreadsheetFormatOptions,
  PreparedOpenDocument,
  FileOperationReceipt,
  FileOperationResultLookup,
} from '@/types/fileRuntime';

export function runtimePreparedOpenDocument(
  prepared: ProtocolPreparedOpenDocument,
): PreparedOpenDocument {
  const session = openSessionState(prepared.preview);
  const manifestResidentBytes = admitDocumentManifestResidentBytes(session.data);
  return {
    token: prepared.token,
    preview: {
      session,
      status: editorSessionStatusState(prepared.preview.editorSession),
      manifestResidentBytes,
    },
  };
}

export function runtimeFileOperationReceipt(
  receipt: ProtocolFileOperationReceipt,
): FileOperationReceipt {
  return { ...receipt };
}

export function runtimeFileOperationResultLookup(
  lookup: ProtocolFileOperationResultLookup,
): FileOperationResultLookup {
  return {
    status: lookup.status,
    receipt: lookup.receipt ? runtimeFileOperationReceipt(lookup.receipt) : undefined,
  };
}

export function fileOperationReceiptFromOpenResponse(
  response: OpenDocumentResponse,
): FileOperationReceipt {
  return {
    kind: 'open',
    documentId: response.editorSession.documentId,
    revision: response.editorSession.revision,
    path: response.document.path,
    fileName: response.document.fileName,
  };
}

export function fileOperationReceiptFromSavedResponse(
  response: SavedDocumentResponse,
): FileOperationReceipt {
  const identity = response.document ?? response.identity;
  if (!identity) {
    throw new Error('Saved document response did not include manifest or identity data');
  }
  return {
    kind: 'save',
    documentId: response.editorSession.documentId,
    revision: response.editorSession.revision,
    path: identity.path,
    fileName: identity.fileName,
  };
}

export function savedResponseFromOpenResponse(
  response: OpenDocumentResponse,
): SavedDocumentResponse {
  return {
    document: response.document,
    editorSession: response.editorSession,
  };
}

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
