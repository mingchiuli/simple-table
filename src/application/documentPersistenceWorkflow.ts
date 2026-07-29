import { createDocumentFileOperationProtocol } from '@/application/documentFileOperationProtocol';
import type {
  DocumentLifecycleRunner,
  ReservedSaveLocation,
} from '@/application/documentFileWorkflow';
import type { DocumentProjection, EditorCommandContext } from '@/types/documentRuntime';
import type {
  DocumentCapabilities,
  FileOperationReceipt,
  FileOperationResultLookup,
  NativeSavePlan,
} from '@/types/fileRuntime';
import { baseNameWithoutExtension, isUntitledSpreadsheet } from '@/utils/fileFormats';
import { isNextU64 } from '@/utils/u64';

export type SaveFileOutcome =
  | { status: 'none' }
  | { status: 'saved' }
  | { status: 'saved-stale' }
  | { status: 'blocked'; message: string };

export type ExportFileOutcome = 'none' | 'exported';

export type DocumentPersistenceWorkflowPorts<ActiveDocument, SavedDocument> = {
  getFileData: () => DocumentProjection | null;
  getCommandContext: () => EditorCommandContext | null;
  getCurrentFilePath: () => string | null;
  getCurrentSheetIndex: () => number;
  runDocumentLifecycle: DocumentLifecycleRunner;
  prepareConsistentContext: () => Promise<EditorCommandContext | null>;
  saveFile: (
    path: string,
    context: EditorCommandContext,
    operationId: string,
  ) => Promise<SavedDocument>;
  getFileOperationResult: (operationId: string) => Promise<FileOperationResultLookup>;
  getActiveDocument: () => Promise<ActiveDocument | null>;
  receiptFromActiveDocument: (document: ActiveDocument) => FileOperationReceipt;
  receiptFromSavedDocument: (document: SavedDocument) => FileOperationReceipt;
  savedDocumentFromActive: (document: ActiveDocument) => SavedDocument;
  exportFile: (
    defaultName: string,
    context: EditorCommandContext,
    operationId: string,
  ) => Promise<FileOperationReceipt | null>;
  nativeSavePlan: (
    context: EditorCommandContext,
    targetPathOrName: string,
  ) => Promise<NativeSavePlan>;
  documentCapabilities: (context: EditorCommandContext) => Promise<DocumentCapabilities>;
  defaultSpreadsheetExtension: () => Promise<string>;
  withReservedSaveLocation: <T>(
    defaultName: string,
    action: (location: ReservedSaveLocation) => Promise<T>,
  ) => Promise<T | null>;
  applySavedDocumentResponse: (
    context: EditorCommandContext,
    response: SavedDocument,
    path: string,
    preferredSheetIndex: number,
  ) => boolean;
  queueRecentFileEntryUpdate: (originalPath?: string) => void;
  reportCleanupError?: (message: string, error: unknown) => void;
};

export function createDocumentPersistenceWorkflow<ActiveDocument, SavedDocument>(
  ports: DocumentPersistenceWorkflowPorts<ActiveDocument, SavedDocument>,
) {
  const fileOperations = createDocumentFileOperationProtocol({
    getFileOperationResult: ports.getFileOperationResult,
    reportError: ports.reportCleanupError,
  });

  async function saveCurrentFile(): Promise<SaveFileOutcome> {
    let outcome: SaveFileOutcome = { status: 'none' };
    await ports.runDocumentLifecycle('saving', async () => {
      const data = ports.getFileData();
      if (!data) return;
      const context = await ports.prepareConsistentContext();
      if (!context) return;

      const isNewFile = isUntitledSpreadsheet(data.fileName);
      const defaultName = isNewFile ? 'untitled' : baseNameWithoutExtension(data.fileName);
      const existingPath = ports.getCurrentFilePath();
      const savePlan = await ports.nativeSavePlan(context, existingPath ?? data.fileName);

      if (existingPath && savePlan.canSave && !savePlan.requiresSaveAs) {
        outcome = await saveToPath(existingPath, context);
        return;
      }
      if (existingPath && !savePlan.requiresSaveAs && !savePlan.canSave) {
        outcome = blockedSaveOutcome(savePlan);
        return;
      }

      await ports.withReservedSaveLocation(
        `${defaultName}.${savePlan.defaultExtension}`,
        async ({ path, markPersisted }) => {
          const targetPlan = await ports.nativeSavePlan(context, path);
          if (!targetPlan.canSave) {
            outcome = blockedSaveOutcome(targetPlan);
            return;
          }
          const saved = await commitSavedDocument(path, context);
          markPersisted();
          outcome = applySavedResponse(path, context, saved);
        },
      );
    });
    return outcome;
  }

  async function exportCurrentFile(): Promise<ExportFileOutcome> {
    let outcome: ExportFileOutcome = 'none';
    await ports.runDocumentLifecycle('saving', async () => {
      const data = ports.getFileData();
      if (!data) return;
      const context = await ports.prepareConsistentContext();
      if (!context) return;

      const isNewFile = isUntitledSpreadsheet(data.fileName);
      const defaultName = isNewFile ? 'untitled' : baseNameWithoutExtension(data.fileName);
      const capabilities = await ports.documentCapabilities(context);
      const extension = isNewFile
        ? await ports.defaultSpreadsheetExtension()
        : capabilities.exportExtension;
      const exported = await fileOperations.execute({
        kind: 'export',
        invoke: (operationId) => ports.exportFile(
          `${defaultName}.${extension}`,
          context,
          operationId,
        ),
        receiptForResponse: (receipt) => receipt,
        validateReceipt: (receipt) => (
          receipt.documentId === context.documentId
          && receipt.revision === context.baseRevision
        ),
        recoverResponse: async (receipt) => receipt,
        recoverCancelled: () => null,
      });
      if (exported) {
        outcome = 'exported';
      }
    });
    return outcome;
  }

  async function saveToPath(
    path: string,
    context: EditorCommandContext,
  ): Promise<SaveFileOutcome> {
    const saved = await commitSavedDocument(path, context);
    return applySavedResponse(path, context, saved);
  }

  async function commitSavedDocument(
    path: string,
    context: EditorCommandContext,
  ): Promise<SavedDocument> {
    return fileOperations.execute({
      kind: 'save',
      invoke: (operationId) => ports.saveFile(path, context, operationId),
      receiptForResponse: ports.receiptFromSavedDocument,
      validateReceipt: (receipt) => receiptMatchesSave(receipt, context),
      recoverResponse: (receipt) => recoverSavedResponse(receipt),
      recoverAmbiguous: async () => {
        const active = await ports.getActiveDocument();
        if (!active) return null;
        const receipt = ports.receiptFromActiveDocument(active);
        return receiptMatchesSave(receipt, context)
          ? ports.savedDocumentFromActive(active)
          : null;
      },
    });
  }

  async function recoverSavedResponse(receipt: FileOperationReceipt): Promise<SavedDocument> {
    const active = await ports.getActiveDocument();
    if (!active) {
      throw new Error('Completed save receipt does not match the active document');
    }
    const activeReceipt = ports.receiptFromActiveDocument(active);
    if (
      activeReceipt.documentId !== receipt.documentId
      || activeReceipt.revision !== receipt.revision
    ) {
      throw new Error('Completed save receipt does not match the active document');
    }
    return ports.savedDocumentFromActive(active);
  }

  function applySavedResponse(
    path: string,
    context: EditorCommandContext,
    saved: SavedDocument,
  ): SaveFileOutcome {
    if (!ports.applySavedDocumentResponse(
      context,
      saved,
      path,
      ports.getCurrentSheetIndex(),
    )) {
      return { status: 'saved-stale' };
    }
    ports.queueRecentFileEntryUpdate();
    return { status: 'saved' };
  }

  return { saveCurrentFile, exportCurrentFile };
}

function blockedSaveOutcome(plan: NativeSavePlan): SaveFileOutcome {
  return {
    status: 'blocked',
    message: plan.blockedReason ?? 'Workbook cannot be saved in its current state.',
  };
}

function receiptMatchesSave(
  receipt: FileOperationReceipt,
  context: EditorCommandContext,
): boolean {
  return receipt.documentId === context.documentId
    && isNextU64(receipt.revision, context.baseRevision);
}
