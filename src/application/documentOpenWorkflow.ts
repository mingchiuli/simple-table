import { createDocumentFileOperationProtocol } from '@/application/documentFileOperationProtocol';
import { invokeIdempotently } from '@/application/idempotentCommandProtocol';
import type {
  DocumentLifecycleRunner,
  DocumentReplacementLease,
  OpenFileSelection,
} from '@/application/documentFileWorkflow';
import {
  createDocumentPreparationCoordinator,
  type DocumentPreparationCoordinator,
} from '@/application/documentPreparationCoordinator';
import {
  neverCancelled,
  type OperationCancellationSignal,
} from '@/application/operationCancellation';
import type { EditorCommandContext } from '@/types/documentRuntime';
import type {
  FileOperationReceipt,
  FileOperationResultLookup,
  PreparedOpenDocument,
} from '@/types/fileRuntime';
import type { RecentFile } from '@/types/recentFileRuntime';

export type DocumentOpenWorkflowPorts<ActiveDocument> = {
  getCommandContext: () => EditorCommandContext | null;
  beginDocumentReplacement: () => Promise<DocumentReplacementLease | null>;
  runDocumentLifecycle: DocumentLifecycleRunner;
  pickOpenFile: () => Promise<OpenFileSelection | null>;
  discardOpenFileSelection: (selection: OpenFileSelection) => Promise<void>;
  prepareOpenFile: (path: string, preparationId: string) => Promise<PreparedOpenDocument>;
  prepareRecentFile: (
    file: RecentFile,
    preparationId: string,
  ) => Promise<PreparedOpenDocument>;
  prepareNewFile: (preparationId: string) => Promise<PreparedOpenDocument>;
  commitPreparedDocument: (
    prepared: PreparedOpenDocument,
    expectedContext: EditorCommandContext | null,
    operationId: string,
  ) => Promise<FileOperationReceipt>;
  getFileOperationResult: (operationId: string) => Promise<FileOperationResultLookup>;
  getActiveDocument: () => Promise<ActiveDocument | null>;
  receiptFromActiveDocument: (document: ActiveDocument) => FileOperationReceipt;
  abortPreparedDocument: (preparationId: string) => Promise<void>;
  closeDocument: (context: EditorCommandContext) => Promise<void>;
  openPreparedDocument: (prepared: PreparedOpenDocument, path: string | null) => void;
  clearDocument: () => void;
  queueRecentFileEntryUpdate: (originalPath?: string) => void;
  reportCleanupError?: (message: string, error: unknown) => void;
};

export function createDocumentOpenWorkflow<ActiveDocument>(
  ports: DocumentOpenWorkflowPorts<ActiveDocument>,
  preparations: DocumentPreparationCoordinator = createDocumentPreparationCoordinator(),
  createPreparationId: () => string = defaultPreparationId,
) {
  const pendingPreparationCleanupIds = new Set<string>();
  const fileOperations = createDocumentFileOperationProtocol({
    getFileOperationResult: ports.getFileOperationResult,
    reportError: ports.reportCleanupError,
  });

  async function loadFileFromPath(
    filePath: string,
    cancellation: OperationCancellationSignal = neverCancelled,
  ): Promise<boolean> {
    let loaded = false;
    await ports.runDocumentLifecycle(
      'loading',
      async () => {
        if (cancellation.isCancelled()) return;
        const replacement = await ports.beginDocumentReplacement();
        if (!replacement) return;
        try {
          if (cancellation.isCancelled()) return;
          const expectedContext = ports.getCommandContext();
          const prepared = await preparations.runCancellable(
            () => prepareDocument((preparationId) => (
              ports.prepareOpenFile(filePath, preparationId)
            )),
            cancellation,
            (result) => ports.abortPreparedDocument(result.token),
          );
          if (!prepared) return;
          if (cancellation.isCancelled()) {
            await abortPreparedDocumentQuietly(prepared);
            return;
          }
          const receipt = await commitPreparedDocument(prepared, expectedContext);
          if (cancellation.isCancelled()) {
            try {
              await ports.closeDocument({
                documentId: receipt.documentId,
                baseRevision: receipt.revision,
              });
              replacement.commit();
              ports.clearDocument();
            } catch (error) {
              replacement.commit();
              ports.openPreparedDocument(prepared, filePath);
              throw error;
            }
            return;
          }
          replacement.commit();
          ports.openPreparedDocument(prepared, filePath);
          loaded = true;
          ports.queueRecentFileEntryUpdate();
        } finally {
          if (!loaded) replacement.cancel();
        }
      },
      { waitForIdle: true, shouldContinue: () => !cancellation.isCancelled() },
    );
    return loaded;
  }

  async function openPickedFile(): Promise<boolean> {
    let opened = false;
    await ports.runDocumentLifecycle('loading', async () => {
      const selection = await ports.pickOpenFile();
      if (!selection) return;
      opened = await openSelectedFileWithinLifecycle(selection);
    });
    return opened;
  }

  async function openSelectedFile(selection: OpenFileSelection): Promise<boolean> {
    let opened = false;
    await ports.runDocumentLifecycle('loading', async () => {
      opened = await openSelectedFileWithinLifecycle(selection);
    });
    return opened;
  }

  async function createNewDocument(): Promise<boolean> {
    let created = false;
    await ports.runDocumentLifecycle('loading', async () => {
      created = await replaceWithPreparedDocument(
        (preparationId) => ports.prepareNewFile(preparationId),
        null,
      );
    });
    return created;
  }

  async function openRecentDocument(file: RecentFile): Promise<boolean> {
    let opened = false;
    await ports.runDocumentLifecycle('loading', async () => {
      opened = await replaceWithPreparedDocument(
        (preparationId) => ports.prepareRecentFile(file, preparationId),
        file.path,
        file.originalPath,
      );
    });
    return opened;
  }

  async function openSelectedFileWithinLifecycle(selection: OpenFileSelection): Promise<boolean> {
    let discardSelection = true;
    let replacement: DocumentReplacementLease | null = null;
    let actionError: unknown;
    try {
      replacement = await ports.beginDocumentReplacement();
      if (!replacement) return false;
      const expectedContext = ports.getCommandContext();
      const prepared = await preparations.run(() => prepareDocument((preparationId) => (
        ports.prepareOpenFile(selection.path, preparationId)
      )));
      await commitPreparedDocument(prepared, expectedContext);
      discardSelection = false;
      replacement.commit();
      replacement = null;
      ports.openPreparedDocument(prepared, selection.path);
      ports.queueRecentFileEntryUpdate(selection.originalPath);
      return true;
    } catch (error) {
      actionError = error;
      throw error;
    } finally {
      replacement?.cancel();
      if (discardSelection) {
        try {
          await ports.discardOpenFileSelection(selection);
        } catch (cleanupError) {
          ports.reportCleanupError?.(
            actionError === undefined
              ? 'Failed to discard unused open file selection'
              : 'Failed to discard open file selection after open error',
            cleanupError,
          );
        }
      }
    }
  }

  async function replaceWithPreparedDocument(
    prepare: (preparationId: string) => Promise<PreparedOpenDocument>,
    path: string | null,
    recentOriginalPath?: string,
  ): Promise<boolean> {
    const replacement = await ports.beginDocumentReplacement();
    if (!replacement) return false;
    try {
      const expectedContext = ports.getCommandContext();
      const prepared = await preparations.run(() => prepareDocument(prepare));
      await commitPreparedDocument(prepared, expectedContext);
      replacement.commit();
      ports.openPreparedDocument(prepared, path);
      if (path !== null) ports.queueRecentFileEntryUpdate(recentOriginalPath);
      return true;
    } catch (error) {
      replacement.cancel();
      throw error;
    }
  }

  async function commitPreparedDocument(
    prepared: PreparedOpenDocument,
    expectedContext: EditorCommandContext | null,
  ): Promise<FileOperationReceipt> {
    try {
      return await fileOperations.execute({
        kind: 'open',
        invoke: (operationId) => ports.commitPreparedDocument(
          prepared,
          expectedContext,
          operationId,
        ),
        receiptForResponse: (receipt) => receipt,
        validateReceipt: (receipt) => receiptMatchesPrepared(receipt, prepared),
        recoverResponse: async (receipt) => receipt,
        recoverAmbiguous: async () => {
          const active = await ports.getActiveDocument();
          if (!active) return null;
          const receipt = ports.receiptFromActiveDocument(active);
          return receiptMatchesPrepared(receipt, prepared) ? receipt : null;
        },
      });
    } catch (error) {
      await abortPreparedDocumentQuietly(prepared);
      throw error;
    }
  }

  async function abortPreparedDocumentQuietly(prepared: PreparedOpenDocument) {
    try {
      await preparations.cleanup(prepared, (result) => ports.abortPreparedDocument(result.token));
    } catch (error) {
      ports.reportCleanupError?.('Failed to abort unused prepared document', error);
    }
  }

  async function prepareDocument(
    prepare: (preparationId: string) => Promise<PreparedOpenDocument>,
  ): Promise<PreparedOpenDocument> {
    await cleanupPendingPreparationIds();
    const preparationId = createPreparationId();
    try {
      const invocation = await invokeIdempotently({
        operationId: preparationId,
        invoke: prepare,
      });
      if (invocation.status === 'ambiguous') throw invocation.error;
      return invocation.response;
    } catch (error) {
      await abortPreparationIdQuietly(preparationId);
      throw error;
    }
  }

  async function abortPreparationIdQuietly(preparationId: string) {
    let lastError: unknown;
    for (let attempt = 0; attempt < 3; attempt += 1) {
      try {
        await ports.abortPreparedDocument(preparationId);
        pendingPreparationCleanupIds.delete(preparationId);
        return true;
      } catch (error) {
        lastError = error;
        if (attempt < 2) await Promise.resolve();
      }
    }
    pendingPreparationCleanupIds.add(preparationId);
    ports.reportCleanupError?.('Failed to abort document preparation', lastError);
    return false;
  }

  async function cleanupPendingPreparationIds() {
    for (const preparationId of [...pendingPreparationCleanupIds]) {
      if (!(await abortPreparationIdQuietly(preparationId))) {
        throw new Error('Previous document preparation could not be cleaned up');
      }
    }
  }

  return {
    loadFileFromPath,
    openPickedFile,
    openSelectedFile,
    createNewDocument,
    openRecentDocument,
  };
}

let fallbackPreparationId = 0;

function defaultPreparationId(): string {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
    return crypto.randomUUID();
  }
  fallbackPreparationId += 1;
  return `prepare-${Date.now()}-${fallbackPreparationId}`;
}

function receiptMatchesPrepared(
  receipt: FileOperationReceipt,
  prepared: PreparedOpenDocument,
): boolean {
  return receipt.documentId === prepared.preview.session.documentId
    && receipt.revision === prepared.preview.session.revision
    && receipt.fileName === prepared.preview.session.data.fileName;
}
