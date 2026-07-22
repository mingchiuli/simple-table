import { describe, expect, it, vi } from 'vitest';

import {
  createDocumentFileCoordinator,
  type DocumentFileCoordinatorPorts,
} from '@/application/documentFileCoordinator';
import {
  defaultWorkbookCapabilities,
  type DocumentProjection,
  type EditorCommandContext,
  type NativeSavePlan,
  type RecentFile,
} from '@/types';
import type { OpenDocumentResponse, SavedDocumentResponse } from '@/types/protocol';
import type { OperationCancellationSignal } from '@/application/operationCancellation';
import { createDocumentPreparationCoordinator } from '@/application/documentPreparationCoordinator';

const context: EditorCommandContext = {
  documentId: '1',
  baseRevision: '0',
};

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
  const ports: TestPorts = {
    getFileData: () => projection(),
    getCommandContext: () => context,
    getCurrentFilePath: () => '/tmp/book.xlsx',
    getCurrentSheetIndex: () => 0,
    beginDocumentReplacement: vi.fn().mockResolvedValue(replacement),
    runDocumentLifecycle: vi.fn(async (_lifecycle, _errorPrefix, action) => {
      try {
        await action({ release: () => undefined });
        return 'completed';
      } catch {
        return 'failed';
      }
    }),
    prepareConsistentContext: vi.fn().mockResolvedValue(context),
    pickOpenFile: vi.fn().mockResolvedValue(null),
    discardOpenFileSelection: vi.fn().mockResolvedValue(undefined),
    prepareOpenFile: vi.fn().mockResolvedValue({ token: 'prepared' }),
    prepareRecentFile: vi.fn().mockResolvedValue({ token: 'recent' }),
    prepareNewFile: vi.fn().mockResolvedValue({ token: 'new' }),
    commitPreparedDocument: vi.fn().mockResolvedValue({} as OpenDocumentResponse),
    openedDocumentId: (opened) => opened.editorSession.documentId,
    abortPreparedDocument: vi.fn().mockResolvedValue(undefined),
    closeDocument: vi.fn().mockResolvedValue(undefined),
    saveFile: vi.fn().mockResolvedValue({} as SavedDocumentResponse),
    exportFile: vi.fn().mockResolvedValue(null),
    nativeSavePlan: vi.fn().mockResolvedValue(savePlan()),
    documentCapabilities: vi.fn().mockResolvedValue(savePlan().capabilities),
    defaultSpreadsheetExtension: vi.fn().mockResolvedValue('xlsx'),
    withReservedSaveLocation: vi.fn().mockResolvedValue(null),
    openDocumentResponse: vi.fn(),
    applySavedDocumentResponse: vi.fn().mockReturnValue(true),
    clearDocument: vi.fn(),
    queueRecentFileEntryUpdate: vi.fn(),
    ...overrides,
  };
  return { ports, replacement };
}

describe('documentFileCoordinator', () => {
  it('aborts a prepared route load when its continuation becomes stale', async () => {
    let current = true;
    const prepareOpenFile = vi.fn(async () => {
      current = false;
      return { token: 'stale' };
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

    expect(ports.abortPreparedDocument).toHaveBeenCalledWith({ token: 'stale' });
    expect(ports.commitPreparedDocument).not.toHaveBeenCalled();
    expect(replacement.cancel).toHaveBeenCalledOnce();
  });

  it('releases a cancelled route load while draining preparation before the next one', async () => {
    const firstPrepare = deferred<{ token: string }>();
    const prepareOpenFile = vi.fn()
      .mockReturnValueOnce(firstPrepare.promise)
      .mockResolvedValueOnce({ token: 'latest' });
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

    firstPrepare.resolve({ token: 'slow' });
    await expect(latest).resolves.toBe(true);
    expect(ports.abortPreparedDocument).toHaveBeenCalledWith({ token: 'slow' });
    expect(prepareOpenFile).toHaveBeenNthCalledWith(2, '/tmp/latest.xlsx');
  });

  it('serializes new-document preparation behind a cancelled route parse', async () => {
    const routePrepare = deferred<{ token: string }>();
    const prepareOpenFile = vi.fn().mockReturnValue(routePrepare.promise);
    const prepareNewFile = vi.fn().mockResolvedValue({ token: 'new' });
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

    routePrepare.resolve({ token: 'stale-route' });

    await expect(createDocument).resolves.toBe(true);
    expect(ports.abortPreparedDocument).toHaveBeenCalledWith({ token: 'stale-route' });
    expect(prepareNewFile).toHaveBeenCalledOnce();
  });

  it('returns a stale outcome when a physical save cannot update the active projection', async () => {
    const applySavedDocumentResponse = vi.fn().mockReturnValue(false);
    const { ports } = createPorts({ applySavedDocumentResponse });
    const coordinator = createDocumentFileCoordinator(ports);

    await expect(coordinator.saveCurrentFile()).resolves.toEqual({ status: 'saved-stale' });

    expect(ports.saveFile).toHaveBeenCalledWith('/tmp/book.xlsx', context);
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

  it('cancels replacement when closing the backend document fails', async () => {
    const closeDocument = vi.fn().mockRejectedValue(new Error('busy'));
    const { ports, replacement } = createPorts({ closeDocument });
    const coordinator = createDocumentFileCoordinator(ports);

    await expect(coordinator.closeCurrentDocument()).resolves.toBe(false);

    expect(replacement.cancel).toHaveBeenCalledOnce();
    expect(ports.clearDocument).not.toHaveBeenCalled();
  });

  it('prepares application exit without closing or clearing the document', async () => {
    const { ports, replacement } = createPorts();
    const coordinator = createDocumentFileCoordinator(ports);

    const preparation = await coordinator.prepareApplicationExit({ waitForIdle: true });

    expect(preparation).not.toBeNull();
    expect(replacement.cancel).not.toHaveBeenCalled();
    expect(replacement.commit).not.toHaveBeenCalled();
    expect(ports.closeDocument).not.toHaveBeenCalled();
    expect(ports.clearDocument).not.toHaveBeenCalled();
    expect(ports.runDocumentLifecycle).toHaveBeenCalledWith(
      'closing',
      'Failed to prepare application exit',
      expect.any(Function),
      { waitForIdle: true },
    );

    preparation?.rollback();
    expect(replacement.cancel).toHaveBeenCalledOnce();
  });

  it('aborts a prepared selected document when commit fails', async () => {
    const commitPreparedDocument = vi.fn().mockRejectedValue(new Error('commit failed'));
    const { ports, replacement } = createPorts({ commitPreparedDocument });
    const coordinator = createDocumentFileCoordinator(ports);

    await expect(coordinator.openSelectedFile(selection)).resolves.toBe(false);

    expect(ports.abortPreparedDocument).toHaveBeenCalledWith({ token: 'prepared' });
    expect(replacement.cancel).toHaveBeenCalledOnce();
    expect(ports.discardOpenFileSelection).toHaveBeenCalledWith(selection);
    expect(ports.openDocumentResponse).not.toHaveBeenCalled();
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

    await expect(coordinator.openSelectedFile(selection)).resolves.toBe(false);

    expect(replacement.cancel).toHaveBeenCalledOnce();
    expect(reportCleanupError).toHaveBeenCalledWith(
      'Failed to discard open file selection after open error',
      expect.any(Error),
    );
  });

  it('commits replacement before publishing a selected document', async () => {
    let replacementCommitted = false;
    const opened = {} as OpenDocumentResponse;
    const openDocumentResponse = vi.fn(() => {
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
      commitPreparedDocument: vi.fn().mockResolvedValue(opened),
      openDocumentResponse,
    });
    const coordinator = createDocumentFileCoordinator(ports);

    await expect(coordinator.openSelectedFile(selection)).resolves.toBe(true);

    expect(openDocumentResponse).toHaveBeenCalledWith(opened, selection.path);
    expect(ports.discardOpenFileSelection).not.toHaveBeenCalled();
    expect(ports.abortPreparedDocument).not.toHaveBeenCalled();
    expect(ports.queueRecentFileEntryUpdate).toHaveBeenCalledWith(selection.originalPath);
  });

  it('aborts a prepared route document when commit fails', async () => {
    const { ports, replacement } = createPorts({
      commitPreparedDocument: vi.fn().mockRejectedValue(new Error('stale context')),
    });
    const coordinator = createDocumentFileCoordinator(ports);

    await expect(coordinator.loadFileFromPath('/tmp/route.xlsx')).resolves.toBe(false);

    expect(ports.abortPreparedDocument).toHaveBeenCalledWith({ token: 'prepared' });
    expect(replacement.cancel).toHaveBeenCalledOnce();
    expect(ports.openDocumentResponse).not.toHaveBeenCalled();
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
    const opened = {} as OpenDocumentResponse;
    const commitPreparedDocument = vi.fn().mockResolvedValue(opened);
    const { ports, replacement } = createPorts({ commitPreparedDocument });
    const coordinator = createDocumentFileCoordinator(ports);

    await expect(coordinator.openRecentDocument(recent)).resolves.toBe(true);

    expect(ports.prepareRecentFile).toHaveBeenCalledWith(recent);
    expect(replacement.commit).toHaveBeenCalledOnce();
    expect(ports.openDocumentResponse).toHaveBeenCalledWith(opened, recent.path);
    expect(ports.queueRecentFileEntryUpdate).toHaveBeenCalledWith(recent.originalPath);
    expect(ports.runDocumentLifecycle).toHaveBeenCalledWith(
      'loading',
      'Failed to open file',
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
      'Failed to open file',
      expect.any(Function),
    );
  });

  it('creates a new document without adding an unsaved recent entry', async () => {
    const opened = {} as OpenDocumentResponse;
    const { ports, replacement } = createPorts({
      commitPreparedDocument: vi.fn().mockResolvedValue(opened),
    });
    const coordinator = createDocumentFileCoordinator(ports);

    await expect(coordinator.createNewDocument()).resolves.toBe(true);

    expect(ports.prepareNewFile).toHaveBeenCalledOnce();
    expect(replacement.commit).toHaveBeenCalledOnce();
    expect(ports.openDocumentResponse).toHaveBeenCalledWith(opened, null);
    expect(ports.queueRecentFileEntryUpdate).not.toHaveBeenCalled();
    expect(ports.runDocumentLifecycle).toHaveBeenCalledWith(
      'loading',
      'Failed to create file',
      expect.any(Function),
    );
  });
});
