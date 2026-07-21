import type {
  DocumentProjection,
  EditorCommandContext,
  U64String,
} from '@/types/documentRuntime';
import type {
  DocumentCapabilities,
  NativeSavePlan,
  PreparedOpenDocument,
} from '@/types/fileRuntime';
import type { RecentFile } from '@/types/recentFileRuntime';
import { baseNameWithoutExtension, isUntitledSpreadsheet } from '@/utils/fileFormats';
import {
  neverCancelled,
  type OperationCancellationSignal,
} from '@/application/operationCancellation';
import {
  createDocumentRoutePreparationScheduler,
  type DocumentRoutePreparationScheduler,
} from '@/application/documentRoutePreparationScheduler';

type DocumentLifecycle = 'loading' | 'saving' | 'closing';
type DocumentLifecycleStatus = 'completed' | 'failed' | 'skipped';

type DocumentReplacementLease = {
  commit: () => void;
  cancel: () => void;
};

type LifecycleController = {
  release: () => void;
};

type LifecycleOptions = {
  waitForIdle?: boolean;
  shouldContinue?: () => boolean;
};

type OpenFileSelection = {
  path: string;
  fileName: string;
  originalPath?: string;
};

type ReservedSaveLocation = {
  path: string;
  markPersisted: () => void;
};

export type SaveFileOutcome =
  | { status: 'none' }
  | { status: 'saved' }
  | { status: 'saved-stale' }
  | { status: 'blocked'; message: string };

export type ExportFileOutcome = 'none' | 'exported';

export type DocumentFileCoordinatorPorts<OpenedDocument, SavedDocument> = {
  getFileData: () => DocumentProjection | null;
  getCommandContext: () => EditorCommandContext | null;
  getCurrentFilePath: () => string | null;
  getCurrentSheetIndex: () => number;
  beginDocumentReplacement: () => Promise<DocumentReplacementLease | null>;
  runDocumentLifecycle: (
    lifecycle: DocumentLifecycle,
    errorPrefix: string,
    action: (controller: LifecycleController) => Promise<void>,
    options?: LifecycleOptions
  ) => Promise<DocumentLifecycleStatus>;
  prepareConsistentContext: () => Promise<EditorCommandContext | null>;
  pickOpenFile: () => Promise<OpenFileSelection | null>;
  discardOpenFileSelection: (selection: OpenFileSelection) => Promise<void>;
  prepareOpenFile: (path: string) => Promise<PreparedOpenDocument>;
  prepareRecentFile: (file: RecentFile) => Promise<PreparedOpenDocument>;
  prepareNewFile: () => Promise<PreparedOpenDocument>;
  commitPreparedDocument: (
    prepared: PreparedOpenDocument,
    expectedContext: EditorCommandContext | null
  ) => Promise<OpenedDocument>;
  openedDocumentId: (opened: OpenedDocument) => U64String;
  abortPreparedDocument: (prepared: PreparedOpenDocument) => Promise<void>;
  closeDocument: (documentId: EditorCommandContext['documentId']) => Promise<void>;
  saveFile: (
    path: string,
    context: EditorCommandContext
  ) => Promise<SavedDocument>;
  exportFile: (defaultName: string, context: EditorCommandContext) => Promise<string | null>;
  nativeSavePlan: (
    context: EditorCommandContext,
    targetPathOrName: string
  ) => Promise<NativeSavePlan>;
  documentCapabilities: (
    context: EditorCommandContext
  ) => Promise<DocumentCapabilities>;
  defaultSpreadsheetExtension: () => Promise<string>;
  withReservedSaveLocation: <T>(
    defaultName: string,
    action: (location: ReservedSaveLocation) => Promise<T>
  ) => Promise<T | null>;
  openDocumentResponse: (response: OpenedDocument, path: string | null) => void;
  applySavedDocumentResponse: (
    context: EditorCommandContext,
    response: SavedDocument,
    path: string,
    preferredSheetIndex: number
  ) => boolean;
  clearDocument: () => void;
  queueRecentFileEntryUpdate: (originalPath?: string) => void;
  reportCleanupError?: (message: string, error: unknown) => void;
};

export function createDocumentFileCoordinator<OpenedDocument, SavedDocument>(
  ports: DocumentFileCoordinatorPorts<OpenedDocument, SavedDocument>,
  routePreparations: DocumentRoutePreparationScheduler = createDocumentRoutePreparationScheduler(),
) {
  async function loadFileFromPath(
    filePath: string,
    cancellation: OperationCancellationSignal = neverCancelled,
  ): Promise<boolean> {
    let loaded = false;
    await ports.runDocumentLifecycle(
      'loading',
      'Failed to open file',
      async () => {
        if (cancellation.isCancelled()) return;
        const replacement = await ports.beginDocumentReplacement();
        if (!replacement) return;
        try {
          if (cancellation.isCancelled()) return;
          const expectedContext = ports.getCommandContext();
          const prepared = await routePreparations.run(
            () => ports.prepareOpenFile(filePath),
            cancellation,
            abortPreparedDocumentQuietly,
          );
          if (!prepared) return;
          if (cancellation.isCancelled()) {
            await abortPreparedDocumentQuietly(prepared);
            return;
          }
          const opened = await commitPreparedDocument(prepared, expectedContext);
          if (cancellation.isCancelled()) {
            try {
              await ports.closeDocument(ports.openedDocumentId(opened));
              replacement.commit();
              ports.clearDocument();
            } catch (error) {
              replacement.commit();
              ports.openDocumentResponse(opened, filePath);
              throw error;
            }
            return;
          }
          replacement.commit();
          ports.openDocumentResponse(opened, filePath);
          loaded = true;
          ports.queueRecentFileEntryUpdate();
        } finally {
          if (!loaded) replacement.cancel();
        }
      },
      { waitForIdle: true, shouldContinue: () => !cancellation.isCancelled() }
    );
    return loaded;
  }

  async function openPickedFile(): Promise<boolean> {
    let openedFile = false;
    await ports.runDocumentLifecycle('loading', 'Failed to open file', async () => {
      const selection = await ports.pickOpenFile();
      if (!selection) return;
      openedFile = await openSelectedFile(selection);
    });
    return openedFile;
  }

  async function createNewDocument(): Promise<boolean> {
    return replaceWithPreparedDocument(
      ports.prepareNewFile,
      null,
    );
  }

  async function openRecentDocument(file: RecentFile): Promise<boolean> {
    return replaceWithPreparedDocument(
      () => ports.prepareRecentFile(file),
      file.path,
      file.originalPath,
    );
  }

  async function saveCurrentFile(): Promise<SaveFileOutcome> {
    let outcome: SaveFileOutcome = { status: 'none' };
    await ports.runDocumentLifecycle('saving', 'Failed to save file', async () => {
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
          const saved = await ports.saveFile(path, context);
          markPersisted();
          outcome = applySavedResponse(path, context, saved);
        }
      );
    });
    return outcome;
  }

  async function exportCurrentFile(): Promise<ExportFileOutcome> {
    let outcome: ExportFileOutcome = 'none';
    await ports.runDocumentLifecycle('saving', 'Failed to export file', async () => {
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
      if (await ports.exportFile(`${defaultName}.${extension}`, context)) {
        outcome = 'exported';
      }
    });
    return outcome;
  }

  async function closeCurrentDocument(options: { waitForIdle?: boolean } = {}): Promise<boolean> {
    let closed = false;
    const lifecycleStatus = await ports.runDocumentLifecycle(
      'closing',
      'Failed to close file',
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
          await ports.closeDocument(context.documentId);
        } catch (error) {
          replacement.cancel();
          throw error;
        }
        replacement.commit();
        ports.clearDocument();
        closed = true;
      },
      { waitForIdle: options.waitForIdle }
    );
    return lifecycleStatus !== 'skipped' && closed;
  }

  async function confirmApplicationExit(
    options: { waitForIdle?: boolean } = {},
  ): Promise<boolean> {
    let confirmed = false;
    const lifecycleStatus = await ports.runDocumentLifecycle(
      'closing',
      'Failed to prepare application exit',
      async () => {
        const replacement = await ports.beginDocumentReplacement();
        if (!replacement) return;
        replacement.cancel();
        confirmed = true;
      },
      { waitForIdle: options.waitForIdle },
    );
    return lifecycleStatus !== 'skipped' && confirmed;
  }

  async function openSelectedFile(selection: OpenFileSelection): Promise<boolean> {
    let discardSelection = true;
    let replacement: DocumentReplacementLease | null = null;
    let actionError: unknown;
    try {
      replacement = await ports.beginDocumentReplacement();
      if (!replacement) return false;
      const expectedContext = ports.getCommandContext();
      const prepared = await ports.prepareOpenFile(selection.path);
      const opened = await commitPreparedDocument(prepared, expectedContext);
      discardSelection = false;
      replacement.commit();
      replacement = null;
      ports.openDocumentResponse(opened, selection.path);
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
            cleanupError
          );
        }
      }
    }
  }

  async function replaceWithPreparedDocument(
    prepare: () => Promise<PreparedOpenDocument>,
    path: string | null,
    recentOriginalPath?: string,
  ): Promise<boolean> {
    const replacement = await ports.beginDocumentReplacement();
    if (!replacement) return false;
    try {
      const expectedContext = ports.getCommandContext();
      const prepared = await prepare();
      const opened = await commitPreparedDocument(prepared, expectedContext);
      replacement.commit();
      ports.openDocumentResponse(opened, path);
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
  ): Promise<OpenedDocument> {
    try {
      return await ports.commitPreparedDocument(prepared, expectedContext);
    } catch (error) {
      await abortPreparedDocumentQuietly(prepared);
      throw error;
    }
  }

  async function saveToPath(
    path: string,
    context: EditorCommandContext
  ): Promise<SaveFileOutcome> {
    const saved = await ports.saveFile(path, context);
    return applySavedResponse(path, context, saved);
  }

  function applySavedResponse(
    path: string,
    context: EditorCommandContext,
    saved: SavedDocument
  ): SaveFileOutcome {
    if (!ports.applySavedDocumentResponse(
      context,
      saved,
      path,
      ports.getCurrentSheetIndex()
    )) {
      return { status: 'saved-stale' };
    }
    ports.queueRecentFileEntryUpdate();
    return { status: 'saved' };
  }

  function blockedSaveOutcome(plan: NativeSavePlan): SaveFileOutcome {
    return {
      status: 'blocked',
      message: plan.blockedReason ?? 'Workbook cannot be saved in its current state.',
    };
  }

  async function abortPreparedDocumentQuietly(prepared: PreparedOpenDocument) {
    try {
      await ports.abortPreparedDocument(prepared);
    } catch (error) {
      ports.reportCleanupError?.('Failed to abort unused prepared document', error);
    }
  }

  return {
    loadFileFromPath,
    openPickedFile,
    openSelectedFile,
    createNewDocument,
    openRecentDocument,
    saveCurrentFile,
    exportCurrentFile,
    closeCurrentDocument,
    confirmApplicationExit,
  };
}
