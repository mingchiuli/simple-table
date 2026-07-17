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
  type OpenDocumentResponse,
  type SavedDocumentResponse,
} from '@/types';

const context: EditorCommandContext = {
  documentId: '1',
  baseRevision: '0',
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

function createPorts(overrides: Partial<DocumentFileCoordinatorPorts> = {}) {
  const replacement = {
    commit: vi.fn(),
    cancel: vi.fn(),
  };
  const ports: DocumentFileCoordinatorPorts = {
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
    commitPreparedDocument: vi.fn().mockResolvedValue({} as OpenDocumentResponse),
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
});
