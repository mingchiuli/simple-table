import { createDocumentFileOperationProtocol } from '@/application/documentFileOperationProtocol';
import type { EditorCommandContext } from '@/types/documentRuntime';
import type {
  FileOperationReceipt,
  FileOperationResultLookup,
} from '@/types/fileRuntime';
import {
  neverCancelled,
  type OperationCancellationSignal,
} from '@/application/operationCancellation';

export type DocumentCloseOperationPorts<ActiveDocument> = {
  commitCloseDocument: (
    context: EditorCommandContext,
    operationId: string,
  ) => Promise<FileOperationReceipt>;
  getFileOperationResult: (operationId: string) => Promise<FileOperationResultLookup>;
  getActiveDocument: () => Promise<ActiveDocument | null>;
  receiptFromActiveDocument: (document: ActiveDocument) => FileOperationReceipt;
  reportCleanupError?: (message: string, error: unknown) => void;
};

export function createDocumentCloseOperation<ActiveDocument>(
  ports: DocumentCloseOperationPorts<ActiveDocument>,
  cancellation: OperationCancellationSignal = neverCancelled,
) {
  const fileOperations = createDocumentFileOperationProtocol({
    getFileOperationResult: ports.getFileOperationResult,
    reportError: ports.reportCleanupError,
    cancellation,
  });

  return async function closeDocument(context: EditorCommandContext): Promise<void> {
    await fileOperations.execute({
      kind: 'close',
      invoke: (operationId) => ports.commitCloseDocument(context, operationId),
      receiptForResponse: (receipt) => receipt,
      validateReceipt: (receipt) => receiptMatchesContext(receipt, context),
      recoverResponse: async (receipt) => receipt,
      recoverAmbiguous: async () => {
        const active = await ports.getActiveDocument();
        if (active) {
          const activeReceipt = ports.receiptFromActiveDocument(active);
          if (activeReceipt.documentId === context.documentId) return null;
        }
        return recoveredCloseReceipt(context);
      },
    });
  };
}

function receiptMatchesContext(
  receipt: FileOperationReceipt,
  context: EditorCommandContext,
): boolean {
  return receipt.documentId === context.documentId
    && receipt.revision === context.baseRevision;
}

function recoveredCloseReceipt(context: EditorCommandContext): FileOperationReceipt {
  return {
    kind: 'close',
    documentId: context.documentId,
    revision: context.baseRevision,
    path: '',
    fileName: '',
  };
}
