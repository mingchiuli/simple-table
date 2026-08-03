import { describe, expect, it, vi } from 'vitest';

import {
  createDocumentFileCoordinator,
  type DocumentFileCoordinatorPorts,
} from '@/application/documentFileCoordinator';
import {
  defaultHistoryStatus,
  defaultWorkbookCapabilities,
  readyFormulaStatus,
  type DocumentProjection,
  type EditorCommandContext,
  type NativeSavePlan,
  type PreparedOpenDocument,
  type RecentFile,
} from '@/types';
import type { OpenDocumentResponse, SavedDocumentResponse } from '@/types/protocol';
import type { OperationCancellationSignal } from '@/application/operationCancellation';
import { createDocumentPreparationCoordinator } from '@/application/documentPreparationCoordinator';
import { preparedOpenDocument } from '@/test/documentFixtures';
import { OperationOutcomeUnknownError } from '@/application/operationOutcome';

const context: EditorCommandContext = {
  documentId: '1',
  baseRevision: '0',
};

function appError(code: string, message: string): Error & { code: string } {
  return Object.assign(new Error(message), { code });
}

const selection = {
  path: '/tmp/imported.xlsx',
  fileName: 'imported.xlsx',
  originalPath: 'content://picked',
};

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((promiseResolve) => {
    resolve = promiseResolve;
  });
  return { promise, resolve };
}

async function flushPromises() {
  for (let index = 0; index < 8; index += 1) await Promise.resolve();
}

function controlledCancellation() {
  let cancelled = false;
  const handlers = new Set<() => void>();
  return {
    signal: {
      isCancelled: () => cancelled,
      onCancel(handler: () => void) {
        if (cancelled) {
          handler();
          return () => undefined;
        }
        handlers.add(handler);
        return () => handlers.delete(handler);
      },
    } satisfies OperationCancellationSignal,
    cancel() {
      cancelled = true;
      for (const handler of handlers) handler();
      handlers.clear();
    },
  };
}

function projection(fileName = 'book.xlsx'): DocumentProjection {
  return {
    path: `/tmp/${fileName}`,
    fileName,
    sheets: [],
  };
}

function openedResponse(
  fileName = 'opened.xlsx',
  documentId: `${bigint}` = '2',
  revision: `${bigint}` = '0',
): OpenDocumentResponse {
  return {
    document: { path: `/tmp/${fileName}`, fileName, sheets: [] },
    editorSession: {
      documentId,
      revision,
      formulaStatus: readyFormulaStatus(),
      capabilities: defaultWorkbookCapabilities(),
      editorState: {
        canUndo: false,
        canRedo: false,
        isDirty: false,
        history: defaultHistoryStatus(),
      },
    },
  };
}

function prepared(token = 'prepared'): PreparedOpenDocument {
  return preparedOpenDocument(openedResponse(), token);
}

function openReceipt(value: PreparedOpenDocument) {
  return {
    kind: 'open' as const,
    documentId: value.preview.session.documentId,
    revision: value.preview.session.revision,
    path: value.preview.session.data.path,
    fileName: value.preview.session.data.fileName,
  };
}

function closeReceipt(value: EditorCommandContext = context) {
  return {
    kind: 'close' as const,
    documentId: value.documentId,
    revision: value.baseRevision,
    path: '/tmp/book.xlsx',
    fileName: 'book.xlsx',
  };
}

function savedResponse(): SavedDocumentResponse {
  return {
    document: { path: '/tmp/book.xlsx', fileName: 'book.xlsx', sheets: [] },
    editorSession: {
      ...openedResponse('book.xlsx', '1', '1').editorSession,
    },
  };
}

function savePlan(overrides: Partial<NativeSavePlan> = {}): NativeSavePlan {
  return {
    canSave: true,
    requiresSaveAs: false,
    nativeSaveExtension: 'xlsx',
    defaultExtension: 'xlsx',
    blockedReason: null,
    capabilities: {
      sourceFormat: 'xlsx',
      canSaveOriginal: true,
      nativeSaveFormat: 'xlsx',
      exportFormats: ['xlsx', 'csv'],
      nativeSaveExtension: 'xlsx',
      exportExtension: 'xlsx',
      requiresSaveAsForNativeSave: false,
      workbook: defaultWorkbookCapabilities(),
    },
    ...overrides,
  };
}

type TestPorts = DocumentFileCoordinatorPorts<OpenDocumentResponse, SavedDocumentResponse>;

function createPorts(overrides: Partial<TestPorts> = {}) {
  const replacement = {
    commit: vi.fn(),
    cancel: vi.fn(),
  };
  const lifecycleRelease = vi.fn();
  const defaultPrepared = prepared();
  const ports: TestPorts = {
    getFileData: () => projection(),
    getCommandContext: () => context,
    getCurrentFilePath: () => '/tmp/book.xlsx',
    getCurrentSheetIndex: () => 0,
    beginDocumentReplacement: vi.fn().mockResolvedValue(replacement),
    runDocumentLifecycle: vi.fn(async (_lifecycle, action) => {
      let released = false;
      let retained = false;
      const release = () => {
        if (released) return;
        released = true;
        lifecycleRelease();
      };
      try {
        await action({
          release,
          retain: () => {
            retained = true;
            return { release };
          },
        });
        return 'completed' as const;
      } finally {
        if (!retained) release();
      }
    }),
    prepareConsistentContext: vi.fn().mockResolvedValue(context),
    pickOpenFile: vi.fn().mockResolvedValue(null),
    discardOpenFileSelection: vi.fn().mockResolvedValue(undefined),
    prepareOpenFile: vi.fn().mockResolvedValue(defaultPrepared),
    prepareRecentFile: vi.fn().mockResolvedValue(prepared('recent')),
    prepareNewFile: vi.fn().mockResolvedValue(prepared('new')),
    commitPreparedDocument: vi.fn().mockResolvedValue(openReceipt(defaultPrepared)),
    getFileOperationResult: vi.fn().mockResolvedValue({ status: 'missing' }),
    getActiveDocument: vi.fn().mockResolvedValue(null),
    receiptFromActiveDocument: (document) => ({
      kind: 'open',
      documentId: document.editorSession.documentId,
      revision: document.editorSession.revision,
      path: document.document.path,
      fileName: document.document.fileName,
    }),
    abortPreparedDocument: vi.fn().mockResolvedValue(undefined),
    commitCloseDocument: vi.fn(async (value) => closeReceipt(value)),
    saveFile: vi.fn().mockResolvedValue(savedResponse()),
    receiptFromSavedDocument: (document) => {
      const identity = document.document ?? document.identity!;
      return {
        kind: 'save',
        documentId: document.editorSession.documentId,
        revision: document.editorSession.revision,
        path: identity.path,
        fileName: identity.fileName,
      };
    },
    savedDocumentFromActive: (document) => ({
      document: document.document,
      editorSession: document.editorSession,
    }),
    exportFile: vi.fn().mockResolvedValue(null),
    nativeSavePlan: vi.fn().mockResolvedValue(savePlan()),
    documentCapabilities: vi.fn().mockResolvedValue(savePlan().capabilities),
    defaultSpreadsheetExtension: vi.fn().mockResolvedValue('xlsx'),
    withReservedSaveLocation: vi.fn().mockResolvedValue(null),
    openPreparedDocument: vi.fn(),
    applySavedDocumentResponse: vi.fn().mockReturnValue(true),
    clearDocument: vi.fn(),
    queueRecentFileEntryUpdate: vi.fn(),
    markDocumentSessionOutcomeUnknown: vi.fn(),
    ...overrides,
  };
  return { ports, replacement, lifecycleRelease };
}

describe('documentFileCoordinator', () => {
  it('aborts a prepared route load when its continuation becomes stale', async () => {
    let current = true;
    const prepareOpenFile = vi.fn(async () => {
      current = false;
      return prepared('stale');
    });
    const { ports, replacement } = createPorts({ prepareOpenFile });
    const preparations = createDocumentPreparationCoordinator();
    const coordinator = createDocumentFileCoordinator(ports, preparations);
    const cancellation: OperationCancellationSignal = {
      isCancelled: () => !current,
      onCancel: () => () => undefined,
    };

    await expect(
      coordinator.loadFileFromPath('/tmp/stale.xlsx', cancellation)
    ).resolves.toBe(false);

    expect(ports.abortPreparedDocument).toHaveBeenCalledWith('stale');
    expect(ports.commitPreparedDocument).not.toHaveBeenCalled();
    expect(replacement.cancel).toHaveBeenCalledOnce();
  });

  it('releases a cancelled route load while draining preparation before the next one', async () => {
    const firstPrepare = deferred<PreparedOpenDocument>();
    const prepareOpenFile = vi.fn()
      .mockReturnValueOnce(firstPrepare.promise)
      .mockResolvedValueOnce(prepared('latest'));
    const { ports } = createPorts({ prepareOpenFile });
    const preparations = createDocumentPreparationCoordinator();
    const coordinator = createDocumentFileCoordinator(ports, preparations);
    const firstCancellation = controlledCancellation();

    const first = coordinator.loadFileFromPath('/tmp/slow.xlsx', firstCancellation.signal);
    await flushPromises();
    firstCancellation.cancel();
    await expect(first).resolves.toBe(false);

    const remountedCoordinator = createDocumentFileCoordinator(ports, preparations);
    const latest = remountedCoordinator.loadFileFromPath('/tmp/latest.xlsx');
    await flushPromises();
    expect(prepareOpenFile).toHaveBeenCalledTimes(1);

    firstPrepare.resolve(prepared('slow'));
    await expect(latest).resolves.toBe(true);
    expect(ports.abortPreparedDocument).toHaveBeenCalledWith('slow');
    expect(prepareOpenFile).toHaveBeenNthCalledWith(2, '/tmp/latest.xlsx', expect.any(String));
  });

  it('serializes new-document preparation behind a cancelled route parse', async () => {
    const routePrepare = deferred<PreparedOpenDocument>();
    const prepareOpenFile = vi.fn().mockReturnValue(routePrepare.promise);
    const prepareNewFile = vi.fn().mockResolvedValue(prepared('new'));
    const { ports } = createPorts({ prepareOpenFile, prepareNewFile });
    const preparations = createDocumentPreparationCoordinator();
    const routeCoordinator = createDocumentFileCoordinator(ports, preparations);
    const cancellation = controlledCancellation();

    const routeLoad = routeCoordinator.loadFileFromPath('/tmp/slow.xlsx', cancellation.signal);
    await flushPromises();
    cancellation.cancel();
    await expect(routeLoad).resolves.toBe(false);

    const homeCoordinator = createDocumentFileCoordinator(ports, preparations);
    const createDocument = homeCoordinator.createNewDocument();
    await flushPromises();
    expect(prepareNewFile).not.toHaveBeenCalled();

    routePrepare.resolve(prepared('stale-route'));

    await expect(createDocument).resolves.toBe(true);
    expect(ports.abortPreparedDocument).toHaveBeenCalledWith('stale-route');
    expect(prepareNewFile).toHaveBeenCalledOnce();
  });

  it('returns a stale outcome when a physical save cannot update the active projection', async () => {
    const applySavedDocumentResponse = vi.fn().mockReturnValue(false);
    const { ports } = createPorts({ applySavedDocumentResponse });
    const coordinator = createDocumentFileCoordinator(ports);

    await expect(coordinator.saveCurrentFile()).resolves.toEqual({ status: 'saved-stale' });

    expect(ports.saveFile).toHaveBeenCalledWith(
      '/tmp/book.xlsx',
      context,
      expect.any(String),
    );
    expect(ports.queueRecentFileEntryUpdate).not.toHaveBeenCalled();
  });

  it('reports a blocked native save without writing', async () => {
    const nativeSavePlan = vi.fn().mockResolvedValue(savePlan({
      canSave: false,
      blockedReason: 'Unsupported workbook feature',
    }));
    const { ports } = createPorts({ nativeSavePlan });
    const coordinator = createDocumentFileCoordinator(ports);

    await expect(coordinator.saveCurrentFile()).resolves.toEqual({
      status: 'blocked',
      message: 'Unsupported workbook feature',
    });

    expect(ports.saveFile).not.toHaveBeenCalled();
    expect(ports.withReservedSaveLocation).not.toHaveBeenCalled();
  });

  it('does not invalidate the document projection when a save outcome stays unknown', async () => {
    vi.useFakeTimers();
    try {
      const markDocumentSessionOutcomeUnknown = vi.fn();
      const { ports } = createPorts({
        saveFile: vi.fn().mockRejectedValue(new Error('response lost')),
        getFileOperationResult: vi.fn().mockResolvedValue({ status: 'pending' }),
        markDocumentSessionOutcomeUnknown,
      });
      const coordinator = createDocumentFileCoordinator(ports);
      const save = coordinator.saveCurrentFile();
      const rejectedSave = expect(save).rejects.toBeInstanceOf(OperationOutcomeUnknownError);

      await vi.advanceTimersByTimeAsync(121_000);
      await rejectedSave;

      expect(markDocumentSessionOutcomeUnknown).not.toHaveBeenCalled();
    } finally {
      vi.useRealTimers();
    }
  });

  it('does not invalidate the document projection when an export outcome stays unknown', async () => {
    vi.useFakeTimers();
    try {
      const markDocumentSessionOutcomeUnknown = vi.fn();
      const { ports } = createPorts({
        exportFile: vi.fn().mockRejectedValue(new Error('response lost')),
        getFileOperationResult: vi.fn().mockResolvedValue({ status: 'pending' }),
        markDocumentSessionOutcomeUnknown,
      });
      const coordinator = createDocumentFileCoordinator(ports);
      const exported = coordinator.exportCurrentFile();
      const rejectedExport = expect(exported).rejects.toBeInstanceOf(OperationOutcomeUnknownError);

      await vi.advanceTimersByTimeAsync(121_000);
      await rejectedExport;

      expect(markDocumentSessionOutcomeUnknown).not.toHaveBeenCalled();
    } finally {
      vi.useRealTimers();
    }
  });

  it('cancels replacement when closing the backend document fails', async () => {
    const failure = { code: 'document_state_invalid', message: 'busy' };
    const commitCloseDocument = vi.fn().mockRejectedValue(failure);
    const { ports, replacement } = createPorts({ commitCloseDocument });
    const coordinator = createDocumentFileCoordinator(ports);

    await expect(coordinator.closeCurrentDocument()).rejects.toBe(failure);

    expect(replacement.cancel).toHaveBeenCalledOnce();
    expect(ports.clearDocument).not.toHaveBeenCalled();
  });

  it('clears the frontend after recovering a lost close response', async () => {
    const commitCloseDocument = vi.fn().mockRejectedValue(new Error('response lost'));
    const getFileOperationResult = vi.fn().mockResolvedValue({
      status: 'completed',
      receipt: closeReceipt(),
    });
    const { ports, replacement } = createPorts({
      commitCloseDocument,
      getFileOperationResult,
    });
    const coordinator = createDocumentFileCoordinator(ports);

    await expect(coordinator.closeCurrentDocument()).resolves.toBe(true);

    expect(commitCloseDocument).toHaveBeenCalledTimes(2);
    expect(commitCloseDocument.mock.calls[0]?.[1]).toBe(
      commitCloseDocument.mock.calls[1]?.[1],
    );
    expect(replacement.commit).toHaveBeenCalledOnce();
    expect(ports.clearDocument).toHaveBeenCalledOnce();
  });

  it('prepares application exit without closing or clearing the document', async () => {
    const { ports, replacement, lifecycleRelease } = createPorts();
    const coordinator = createDocumentFileCoordinator(ports);

    const preparation = await coordinator.prepareApplicationExit({ waitForIdle: true });

    expect(preparation).not.toBeNull();
    expect(replacement.cancel).not.toHaveBeenCalled();
    expect(replacement.commit).not.toHaveBeenCalled();
    expect(ports.commitCloseDocument).not.toHaveBeenCalled();
    expect(ports.clearDocument).not.toHaveBeenCalled();
    expect(lifecycleRelease).not.toHaveBeenCalled();
    expect(ports.runDocumentLifecycle).toHaveBeenCalledWith(
      'closing',
      expect.any(Function),
      { waitForIdle: true },
    );

    preparation?.rollback();
    expect(replacement.cancel).toHaveBeenCalledOnce();
    expect(lifecycleRelease).toHaveBeenCalledOnce();

    preparation?.commit();
    expect(replacement.commit).not.toHaveBeenCalled();
    expect(lifecycleRelease).toHaveBeenCalledOnce();
  });

  it('aborts a prepared selected document when commit fails', async () => {
    const commitPreparedDocument = vi.fn().mockRejectedValue(
      appError('document_state_invalid', 'commit failed'),
    );
    const { ports, replacement } = createPorts({ commitPreparedDocument });
    const coordinator = createDocumentFileCoordinator(ports);

    await expect(coordinator.openSelectedFile(selection)).rejects.toThrow('commit failed');

    expect(ports.abortPreparedDocument).toHaveBeenCalledWith('prepared');
    expect(replacement.cancel).toHaveBeenCalledOnce();
    expect(ports.discardOpenFileSelection).toHaveBeenCalledWith(selection);
    expect(ports.openPreparedDocument).not.toHaveBeenCalled();
  });

  it('discards a selected file when document replacement is denied', async () => {
    const reportCleanupError = vi.fn();
    const { ports } = createPorts({
      beginDocumentReplacement: vi.fn().mockResolvedValue(null),
      discardOpenFileSelection: vi.fn().mockRejectedValue(new Error('cleanup failed')),
      reportCleanupError,
    });
    const coordinator = createDocumentFileCoordinator(ports);

    await expect(coordinator.openSelectedFile(selection)).resolves.toBe(false);

    expect(ports.prepareOpenFile).not.toHaveBeenCalled();
    expect(ports.discardOpenFileSelection).toHaveBeenCalledWith(selection);
    expect(reportCleanupError).toHaveBeenCalledWith(
      'Failed to discard unused open file selection',
      expect.any(Error),
    );
  });

  it('contains a selected-file read failure when cleanup also fails', async () => {
    const reportCleanupError = vi.fn();
    const { ports, replacement } = createPorts({
      prepareOpenFile: vi.fn().mockRejectedValue(new Error('broken file')),
      discardOpenFileSelection: vi.fn().mockRejectedValue(new Error('cleanup failed')),
      reportCleanupError,
    });
    const coordinator = createDocumentFileCoordinator(ports);

    await expect(coordinator.openSelectedFile(selection)).rejects.toThrow('broken file');

    expect(replacement.cancel).toHaveBeenCalledOnce();
    expect(reportCleanupError).toHaveBeenCalledWith(
      'Failed to discard open file selection after open error',
      expect.any(Error),
    );
  });

  it('commits replacement before publishing a selected document', async () => {
    let replacementCommitted = false;
    const selectedPrepared = prepared();
    const openPreparedDocument = vi.fn(() => {
      expect(replacementCommitted).toBe(true);
    });
    const cancel = vi.fn();
    const { ports } = createPorts({
      beginDocumentReplacement: vi.fn().mockResolvedValue({
        commit: vi.fn(() => {
          replacementCommitted = true;
        }),
        cancel,
      }),
      prepareOpenFile: vi.fn().mockResolvedValue(selectedPrepared),
      commitPreparedDocument: vi.fn().mockResolvedValue(openReceipt(selectedPrepared)),
      openPreparedDocument,
    });
    const coordinator = createDocumentFileCoordinator(ports);

    await expect(coordinator.openSelectedFile(selection)).resolves.toBe(true);

    expect(openPreparedDocument).toHaveBeenCalledWith(selectedPrepared, selection.path);
    expect(ports.discardOpenFileSelection).not.toHaveBeenCalled();
    expect(ports.abortPreparedDocument).not.toHaveBeenCalled();
    expect(ports.queueRecentFileEntryUpdate).toHaveBeenCalledWith(
      openReceipt(selectedPrepared),
      selection.originalPath,
    );
  });

  it('aborts a prepared route document when commit fails', async () => {
    const { ports, replacement } = createPorts({
      commitPreparedDocument: vi.fn().mockRejectedValue(
        appError('document_state_invalid', 'stale context'),
      ),
    });
    const coordinator = createDocumentFileCoordinator(ports);

    await expect(coordinator.loadFileFromPath('/tmp/route.xlsx')).rejects.toThrow('stale context');

    expect(ports.abortPreparedDocument).toHaveBeenCalledWith('prepared');
    expect(replacement.cancel).toHaveBeenCalledOnce();
    expect(ports.openPreparedDocument).not.toHaveBeenCalled();
  });

  it('opens a recent document through the shared replacement protocol', async () => {
    const recent = {
      id: 'recent-1',
      path: '/tmp/recent.xlsx',
      fileName: 'recent.xlsx',
      lastOpened: 1,
      fileSize: 1,
      thumbnail: undefined,
      storageType: 'desktopPath',
      originalPath: '/original/recent.xlsx',
    } as RecentFile;
    const recentPrepared = prepared('recent');
    const commitPreparedDocument = vi.fn().mockResolvedValue(openReceipt(recentPrepared));
    const { ports, replacement } = createPorts({
      prepareRecentFile: vi.fn().mockResolvedValue(recentPrepared),
      commitPreparedDocument,
    });
    const coordinator = createDocumentFileCoordinator(ports);

    await expect(coordinator.openRecentDocument(recent)).resolves.toBe(true);

    expect(ports.prepareRecentFile).toHaveBeenCalledWith(recent, expect.any(String));
    expect(replacement.commit).toHaveBeenCalledOnce();
    expect(ports.openPreparedDocument).toHaveBeenCalledWith(recentPrepared, recent.path);
    expect(ports.queueRecentFileEntryUpdate).toHaveBeenCalledWith(
      openReceipt(recentPrepared),
      recent.originalPath,
    );
    expect(ports.runDocumentLifecycle).toHaveBeenCalledWith(
      'loading',
      expect.any(Function),
    );
  });

  it('releases the lifecycle before surfacing a recent-file failure', async () => {
    const error = new Error('File not found: /tmp/missing.xlsx');
    const { ports } = createPorts({
      prepareRecentFile: vi.fn().mockRejectedValue(error),
    });
    const coordinator = createDocumentFileCoordinator(ports);

    await expect(coordinator.openRecentDocument({
      id: 'missing',
      path: '/tmp/missing.xlsx',
      fileName: 'missing.xlsx',
      lastOpened: 1,
      fileSize: 1,
      storageType: 'desktopPath',
    })).rejects.toBe(error);

    expect(ports.runDocumentLifecycle).toHaveBeenCalledWith(
      'loading',
      expect.any(Function),
    );
  });

  it('creates a new document without adding an unsaved recent entry', async () => {
    const newPrepared = prepared('new');
    const { ports, replacement } = createPorts({
      prepareNewFile: vi.fn().mockResolvedValue(newPrepared),
      commitPreparedDocument: vi.fn().mockResolvedValue(openReceipt(newPrepared)),
    });
    const coordinator = createDocumentFileCoordinator(ports);

    await expect(coordinator.createNewDocument()).resolves.toBe(true);

    expect(ports.prepareNewFile).toHaveBeenCalledOnce();
    expect(replacement.commit).toHaveBeenCalledOnce();
    expect(ports.openPreparedDocument).toHaveBeenCalledWith(newPrepared, null);
    expect(ports.queueRecentFileEntryUpdate).not.toHaveBeenCalled();
    expect(ports.runDocumentLifecycle).toHaveBeenCalledWith(
      'loading',
      expect.any(Function),
    );
  });

  it('retries a lost preparation response with the same client preparation id', async () => {
    const prepareNewFile = vi.fn()
      .mockRejectedValueOnce(new Error('response lost'))
      .mockImplementation(async (preparationId: string) => prepared(preparationId));
    const { ports } = createPorts({
      prepareNewFile,
      commitPreparedDocument: vi.fn(async (value) => openReceipt(value)),
    });
    const coordinator = createDocumentFileCoordinator(ports);

    await expect(coordinator.createNewDocument()).resolves.toBe(true);

    expect(prepareNewFile).toHaveBeenCalledTimes(2);
    expect(prepareNewFile.mock.calls[0][0]).toBe(prepareNewFile.mock.calls[1][0]);
    expect(ports.abortPreparedDocument).not.toHaveBeenCalled();
  });

  it('aborts a known preparation id when both preparation responses are lost', async () => {
    const transportError = new Error('response lost');
    const prepareNewFile = vi.fn().mockRejectedValue(transportError);
    const { ports } = createPorts({ prepareNewFile });
    const coordinator = createDocumentFileCoordinator(ports);

    await expect(coordinator.createNewDocument()).rejects.toBe(transportError);

    expect(prepareNewFile).toHaveBeenCalledTimes(2);
    const preparationId = prepareNewFile.mock.calls[0][0];
    expect(prepareNewFile.mock.calls[1][0]).toBe(preparationId);
    expect(ports.abortPreparedDocument).toHaveBeenCalledWith(preparationId);
  });

  it('cleans a retained preparation id before admitting the next preparation', async () => {
    const transportError = new Error('response lost');
    const prepareNewFile = vi.fn()
      .mockRejectedValueOnce(transportError)
      .mockRejectedValueOnce(transportError)
      .mockImplementation(async (preparationId: string) => prepared(preparationId));
    const abortPreparedDocument = vi.fn()
      .mockRejectedValueOnce(new Error('cleanup transport lost'))
      .mockRejectedValueOnce(new Error('cleanup transport lost'))
      .mockRejectedValueOnce(new Error('cleanup transport lost'))
      .mockResolvedValue(undefined);
    const { ports } = createPorts({
      prepareNewFile,
      abortPreparedDocument,
      commitPreparedDocument: vi.fn(async (value) => openReceipt(value)),
    });
    const coordinator = createDocumentFileCoordinator(ports);

    await expect(coordinator.createNewDocument()).rejects.toBe(transportError);
    await expect(coordinator.createNewDocument()).resolves.toBe(true);

    const retainedId = prepareNewFile.mock.calls[0][0];
    expect(prepareNewFile.mock.calls[1][0]).toBe(retainedId);
    expect(prepareNewFile.mock.calls[2][0]).not.toBe(retainedId);
    expect(abortPreparedDocument).toHaveBeenNthCalledWith(4, retainedId);
  });

  it('retains preparation cleanup across file coordinator instances', async () => {
    const transportError = new Error('response lost');
    const prepareNewFile = vi.fn()
      .mockRejectedValueOnce(transportError)
      .mockRejectedValueOnce(transportError)
      .mockImplementation(async (preparationId: string) => prepared(preparationId));
    const abortPreparedDocument = vi.fn()
      .mockRejectedValueOnce(new Error('cleanup transport lost'))
      .mockRejectedValueOnce(new Error('cleanup transport lost'))
      .mockRejectedValueOnce(new Error('cleanup transport lost'))
      .mockResolvedValue(undefined);
    const { ports } = createPorts({
      prepareNewFile,
      abortPreparedDocument,
      commitPreparedDocument: vi.fn(async (value) => openReceipt(value)),
    });
    const preparations = createDocumentPreparationCoordinator();
    const first = createDocumentFileCoordinator(ports, preparations);

    await expect(first.createNewDocument()).rejects.toBe(transportError);

    const second = createDocumentFileCoordinator(ports, preparations);
    await expect(second.createNewDocument()).resolves.toBe(true);

    const retainedId = prepareNewFile.mock.calls[0][0];
    expect(prepareNewFile.mock.calls[1][0]).toBe(retainedId);
    expect(prepareNewFile.mock.calls[2][0]).not.toBe(retainedId);
    expect(abortPreparedDocument).toHaveBeenNthCalledWith(4, retainedId);
  });

  it('recovers an exported file after both command responses are lost', async () => {
    const receipt = {
      kind: 'export' as const,
      documentId: context.documentId,
      revision: context.baseRevision,
      path: '/tmp/export.xlsx',
      fileName: 'book.xlsx',
    };
    const exportFile = vi.fn().mockRejectedValue(new Error('response lost'));
    const getFileOperationResult = vi.fn()
      .mockResolvedValueOnce({ status: 'pending' })
      .mockResolvedValueOnce({ status: 'completed', receipt });
    const { ports } = createPorts({ exportFile, getFileOperationResult });
    const coordinator = createDocumentFileCoordinator(ports);

    await expect(coordinator.exportCurrentFile()).resolves.toBe('exported');

    expect(exportFile).toHaveBeenCalledTimes(2);
    expect(exportFile.mock.calls[0][2]).toBe(exportFile.mock.calls[1][2]);
    expect(getFileOperationResult).toHaveBeenCalledWith(exportFile.mock.calls[0][2]);
  });
});
