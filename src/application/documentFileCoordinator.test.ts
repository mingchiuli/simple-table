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

const context: EditorCommandContext = {
  documentId: '1',
  baseRevision: '0',
};

const selection = {
  path: '/tmp/imported.xlsx',
  fileName: 'imported.xlsx',
  originalPath: 'content://picked',
};

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
    const coordinator = createDocumentFileCoordinator(ports);

    await expect(
      coordinator.loadFileFromPath('/tmp/stale.xlsx', () => current)
    ).resolves.toBe(false);

    expect(ports.abortPreparedDocument).toHaveBeenCalledWith({ token: 'stale' });
    expect(ports.commitPreparedDocument).not.toHaveBeenCalled();
    expect(replacement.cancel).toHaveBeenCalledOnce();
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

  it('aborts a prepared document when commit fails', async () => {
    const commitPreparedDocument = vi.fn().mockRejectedValue(new Error('commit failed'));
    const { ports, replacement } = createPorts({ commitPreparedDocument });
    const coordinator = createDocumentFileCoordinator(ports);

    await expect(coordinator.openSelectedFile(selection)).rejects.toThrow('commit failed');

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

  it('preserves a read failure when selected-file cleanup also fails', async () => {
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
  });
});
