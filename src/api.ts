import { invokeCommand } from "@/tauriInvoke";
import {
  runtimeFileOperationReceipt,
  runtimeFileOperationResultLookup,
  runtimePreparedOpenDocument,
} from '@/application/fileProtocol';
import type {
  RecentFile,
  SetCellRequest,
  SearchResponse,
  EditorMutationResponse,
  EditorSessionInfo,
  DocumentCapabilities,
  NativeSavePlan,
  OpenDocumentResponse,
  SpreadsheetFormatOptions,
  EditorCommandContext,
  SearchScope,
  AddRecentFileRequest,
  SheetRegion,
  SheetRegionProjectionResponse,
  U64String,
  MutationResultLookup,
} from '@/types/protocol';
import type { MutationCommandContext } from '@/types/documentRuntime';
import type {
  FileOperationReceipt,
  FileOperationResultLookup,
  PreparedOpenDocument as RuntimePreparedOpenDocument,
} from '@/types/fileRuntime';

export async function prepareNewFile(
  preparationId: string,
): Promise<RuntimePreparedOpenDocument> {
  return runtimePreparedOpenDocument(await invokeCommand("prepare_new_file", { preparationId }));
}

export async function commitPreparedDocument(
  token: string,
  expectedContext: EditorCommandContext | null,
  operationId: string,
): Promise<FileOperationReceipt> {
  return runtimeFileOperationReceipt(await invokeCommand("commit_prepared_document", {
    token,
    expectedDocumentId: expectedContext?.documentId ?? null,
    expectedRevision: expectedContext?.baseRevision ?? null,
    operationId,
  }));
}

export async function getFileOperationResult(
  operationId: string,
): Promise<FileOperationResultLookup> {
  return runtimeFileOperationResultLookup(
    await invokeCommand('get_file_operation_result', { operationId }),
  );
}

export async function abortPreparedDocument(token: string): Promise<void> {
  return invokeCommand("abort_prepared_document", { token });
}

export async function getActiveDocument(): Promise<OpenDocumentResponse | null> {
  return invokeCommand("get_active_document", {});
}

export async function getMutationResult(
  documentId: U64String,
  commandId: string
): Promise<MutationResultLookup> {
  return invokeCommand("get_mutation_result", { documentId, commandId });
}

export async function getCurrentDocumentProjection(
  context: EditorCommandContext,
  preferredSheetIndex: number
): Promise<OpenDocumentResponse> {
  return invokeCommand("get_current_document_projection", { ...context, preferredSheetIndex });
}

export async function getSheetRegionProjection(
  context: EditorCommandContext,
  region: SheetRegion
): Promise<SheetRegionProjectionResponse> {
  return invokeCommand("get_sheet_region_projection", { ...context, region });
}

export async function closeCurrentDocument(
  context: EditorCommandContext,
  operationId: string,
): Promise<FileOperationReceipt> {
  return runtimeFileOperationReceipt(await invokeCommand("close_current_document", {
    ...context,
    operationId,
  }));
}

export async function getDocumentCapabilities(
  context: EditorCommandContext
): Promise<DocumentCapabilities> {
  return invokeCommand("get_document_capabilities", context);
}

export async function getNativeSavePlan(
  context: EditorCommandContext,
  targetPathOrName: string
): Promise<NativeSavePlan> {
  return invokeCommand("get_native_save_plan", { ...context, targetPathOrName });
}

export async function getSpreadsheetFormatOptions(): Promise<SpreadsheetFormatOptions> {
  return invokeCommand("get_spreadsheet_format_options", {});
}

// ==================== Editor Operations ====================

export async function getEditorState(
  context: EditorCommandContext | null = null
): Promise<EditorSessionInfo | null> {
  return invokeCommand("get_editor_state", {
    documentId: context?.documentId ?? null,
    baseRevision: context?.baseRevision ?? null,
  });
}

export async function undo(context: MutationCommandContext): Promise<EditorMutationResponse> {
  return invokeCommand("undo", context);
}

export async function redo(context: MutationCommandContext): Promise<EditorMutationResponse> {
  return invokeCommand("redo", context);
}

// ==================== Cell Operations ====================

export async function setCell(
  context: MutationCommandContext,
  sheetIndex: number,
  row: number,
  col: number,
  text: string
): Promise<EditorMutationResponse> {
  return invokeCommand("set_cell", { ...context, sheetIndex, row, col, text });
}

export async function setCells(
  context: MutationCommandContext,
  changes: SetCellRequest[]
): Promise<EditorMutationResponse> {
  return invokeCommand("set_cells", { ...context, changes });
}

export async function addRow(
  context: MutationCommandContext,
  sheetIndex: number,
  rowIndex: number
): Promise<EditorMutationResponse> {
  return invokeCommand("add_row", { ...context, sheetIndex, rowIndex });
}

export async function deleteRow(
  context: MutationCommandContext,
  sheetIndex: number,
  rowIndex: number
): Promise<EditorMutationResponse> {
  return invokeCommand("delete_row", { ...context, sheetIndex, rowIndex });
}

export async function addColumn(
  context: MutationCommandContext,
  sheetIndex: number,
  colIndex: number
): Promise<EditorMutationResponse> {
  return invokeCommand("add_column", { ...context, sheetIndex, colIndex });
}

export async function deleteColumn(
  context: MutationCommandContext,
  sheetIndex: number,
  colIndex: number
): Promise<EditorMutationResponse> {
  return invokeCommand("delete_column", { ...context, sheetIndex, colIndex });
}

export async function setColumnWidth(
  context: MutationCommandContext,
  sheetIndex: number,
  colIndex: number,
  width: number | null
): Promise<EditorMutationResponse> {
  return invokeCommand("set_column_width", {
    ...context,
    sheetIndex,
    colIndex,
    width,
  });
}

export async function setRowHeight(
  context: MutationCommandContext,
  sheetIndex: number,
  rowIndex: number,
  height: number | null
): Promise<EditorMutationResponse> {
  return invokeCommand("set_row_height", {
    ...context,
    sheetIndex,
    rowIndex,
    height,
  });
}

// ==================== Sheet Operations ====================

export async function addSheet(context: MutationCommandContext): Promise<EditorMutationResponse> {
  return invokeCommand("add_sheet", context);
}

export async function deleteSheet(
  context: MutationCommandContext,
  sheetIndex: number
): Promise<EditorMutationResponse> {
  return invokeCommand("delete_sheet", { ...context, sheetIndex });
}

// ==================== Search Operations ====================

export async function search(
  context: EditorCommandContext,
  query: string,
  scope: SearchScope,
  currentSheetIndex: number | null
): Promise<SearchResponse> {
  return invokeCommand("search", { ...context, query, scope, currentSheetIndex });
}

// ==================== Recent Files Operations ====================

export async function getRecentFiles(): Promise<RecentFile[]> {
  return invokeCommand("get_recent_files", {});
}

export async function addRecentFileWithThumbnail(
  receipt: FileOperationReceipt,
  originalPath?: string
): Promise<RecentFile> {
  const request: AddRecentFileRequest = {
    originalPath,
    documentId: receipt.documentId,
    baseRevision: receipt.revision,
    path: receipt.path,
    fileName: receipt.fileName,
  };

  return invokeCommand("add_recent_file_with_thumbnail", {
    request,
  });
}

export async function removeRecentFile(id: string): Promise<void> {
  return invokeCommand("remove_recent_file", { id });
}
