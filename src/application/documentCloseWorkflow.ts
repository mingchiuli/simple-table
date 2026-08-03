import type { ApplicationExitPreparation } from '@/application/applicationExitCoordinator';
import { createDocumentFileOperationProtocol } from '@/application/documentFileOperationProtocol';
import type {
  DocumentLifecycleRunner,
  DocumentReplacementLease,
} from '@/application/documentFileWorkflow';
import type { EditorCommandContext } from '@/types/documentRuntime';
import type {
  FileOperationReceipt,
  FileOperationResultLookup,
} from '@/types/fileRuntime';
import {
  neverCancelled,
  type OperationCancellationSignal,
} from '@/application/operationCancellation';

export type DocumentCloseWorkflowPorts<ActiveDocument> = {
  getCommandContext: () => EditorCommandContext | null;
  beginDocumentReplacement: () => Promise<DocumentReplacementLease | null>;
  runDocumentLifecycle: DocumentLifecycleRunner;
  commitCloseDocument: (
    context: EditorCommandContext,
    operationId: string,
  ) => Promise<FileOperationReceipt>;
  getFileOperationResult: (operationId: string) => Promise<FileOperationResultLookup>;
  getActiveDocument: () => Promise<ActiveDocument | null>;
  receiptFromActiveDocument: (document: ActiveDocument) => FileOperationReceipt;
  clearDocument: () => void;
  reportCleanupError?: (message: string, error: unknown) => void;
  markDocumentSessionOutcomeUnknown?: (context: EditorCommandContext) => void;
};

export function createDocumentCloseWorkflow<ActiveDocument>(
  ports: DocumentCloseWorkflowPorts<ActiveDocument>,
  cancellation: OperationCancellationSignal = neverCancelled,
) {
  const fileOperations = createDocumentFileOperationProtocol({
    getFileOperationResult: ports.getFileOperationResult,
    reportError: ports.reportCleanupError,
    cancellation,
  });

  async function closeDocument(context: EditorCommandContext): Promise<void> {
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
      onOutcomeUnknown: () => ports.markDocumentSessionOutcomeUnknown?.(context),
    });
  }

  async function closeCurrentDocument(options: { waitForIdle?: boolean } = {}): Promise<boolean> {
    let closed = false;
    const lifecycleStatus = await ports.runDocumentLifecycle(
      'closing',
      async () => {
        const replacement = await ports.beginDocumentReplacement();
        if (!replacement) return;
        const context = ports.getCommandContext();
        if (!context) {
          replacement.commit();
          ports.clearDocument();
          closed = true;
          return;
        }
        try {
          await closeDocument(context);
        } catch (error) {
          replacement.cancel();
          throw error;
        }
        replacement.commit();
        ports.clearDocument();
        closed = true;
      },
      { waitForIdle: options.waitForIdle },
    );
    return lifecycleStatus === 'completed' && closed;
  }

  async function prepareApplicationExit(
    options: { waitForIdle?: boolean } = {},
  ): Promise<ApplicationExitPreparation | null> {
    let preparation: ApplicationExitPreparation | null = null;
    const lifecycleStatus = await ports.runDocumentLifecycle(
      'closing',
      async ({ retain }) => {
        const replacement = await ports.beginDocumentReplacement();
        if (!replacement) return;
        const lifecycleLease = retain();
        let settled = false;
        const settle = (action: () => void) => {
          if (settled) return;
          settled = true;
          try {
            action();
          } finally {
            lifecycleLease.release();
          }
        };
        preparation = {
          commit: () => settle(replacement.commit),
          rollback: () => settle(replacement.cancel),
        };
      },
      { waitForIdle: options.waitForIdle },
    );
    return lifecycleStatus === 'completed' ? preparation : null;
  }

  return { closeDocument, closeCurrentDocument, prepareApplicationExit };
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
