import { describe, expect, it, vi } from 'vitest';

import {
  createDocumentCommandCoordinator,
  type DocumentCommandSessionPort,
  type DocumentCommandStatePort,
  type DocumentCommandTransport,
} from '@/application/documentCommandCoordinator';
import {
  defaultHistoryStatus,
  defaultWorkbookCapabilities,
  readyFormulaStatus,
} from '@/types';
import type { EditorMutationResponse } from '@/types/protocol';

function mutationResponse(): EditorMutationResponse {
  return {
    protocolVersion: 4,
    documentId: '1',
    revision: '1',
    editorState: {
      canUndo: true,
      canRedo: false,
      isDirty: true,
      history: defaultHistoryStatus(),
    },
    formulaStatus: readyFormulaStatus(),
    capabilities: defaultWorkbookCapabilities(),
    patches: [],
  };
}

function setup() {
  const context = { documentId: '1' as const, baseRevision: '0' as const };
  const document: DocumentCommandStatePort = {
    data: null,
    documentId: '1',
    projectionStale: false,
    currentCommandContext: () => context,
    commandContextForDocument: (documentId) => documentId === '1' ? context : null,
    matchesCommandContext: (candidate) => candidate.documentId === '1',
  };
  const release = vi.fn();
  const applyMutationResponseWithResync = vi.fn().mockResolvedValue({
    data: null,
    resyncRequired: false,
    applied: true,
  });
  const refreshAfterMutationFailure = vi.fn().mockResolvedValue(undefined);
  const applyEditorSessionForContext = vi.fn();
  const session: DocumentCommandSessionPort = {
    beginEditorCommand: () => release,
    enqueueDocumentMutation: async (_documentId, task) =>
      task(context, { isCurrent: () => true }),
    waitForMutations: () => Promise.resolve(),
    recoverActiveDocumentResponse: () => false,
    applyMutationResponseWithResync,
    markProjectionStaleFromMutationResponse: () => true,
    refreshAfterMutationFailure,
    applyEditorSessionForContext,
    ensureSheetLoaded: async () => true,
    ensureSheetRegionLoaded: async () => true,
  };
  const transport: DocumentCommandTransport = {
    getMutationResult: async () => ({ status: 'missing' }),
    getActiveDocument: async () => null,
    getCurrentDocumentProjection: async () => {
      throw new Error('projection not configured');
    },
    getEditorState: async () => null,
    getSheetRegionProjection: async () => {
      throw new Error('region not configured');
    },
  };
  const coordinator = createDocumentCommandCoordinator({
    document,
    session,
    transport,
    preferredSheetIndex: () => 0,
  });
  return {
    coordinator,
    context,
    document,
    release,
    applyMutationResponseWithResync,
    refreshAfterMutationFailure,
    applyEditorSessionForContext,
    transport,
  };
}

describe('document command coordinator', () => {
  it('reports post-apply callback failures without treating the mutation as failed', async () => {
    const { coordinator, release, applyMutationResponseWithResync } = setup();
    const callbackError = new Error('selection failed');

    const outcome = await coordinator.runInteractiveMutation({
      action: async () => mutationResponse(),
      flushPendingChanges: async () => true,
      afterApplied: () => {
        throw callbackError;
      },
    });

    expect(outcome).toEqual({ status: 'after-applied-failed', error: callbackError });
    expect(applyMutationResponseWithResync).toHaveBeenCalledOnce();
    expect(release).toHaveBeenCalledOnce();
  });

  it('refreshes the session after an unrecoverable command transport failure', async () => {
    const { coordinator, refreshAfterMutationFailure, release } = setup();
    const commandError = new Error('response channel closed');

    const outcome = await coordinator.runInteractiveMutation({
      action: async () => {
        throw commandError;
      },
      flushPendingChanges: async () => true,
    });

    expect(outcome).toEqual({ status: 'failed', error: commandError });
    expect(refreshAfterMutationFailure).toHaveBeenCalledOnce();
    expect(release).toHaveBeenCalledOnce();
  });

  it('applies editor state only to the context that requested it', async () => {
    const { coordinator, context, applyEditorSessionForContext } = setup();

    const outcome = await coordinator.refreshEditorState();

    expect(outcome).toEqual({ status: 'completed' });
    expect(applyEditorSessionForContext).toHaveBeenCalledWith(context, null);
  });

  it('reports an editor state transport failure for the current context', async () => {
    const { coordinator, transport, applyEditorSessionForContext } = setup();
    const error = new Error('status unavailable');
    transport.getEditorState = vi.fn().mockRejectedValue(error);

    const outcome = await coordinator.refreshEditorState();

    expect(outcome).toEqual({ status: 'failed', error });
    expect(applyEditorSessionForContext).not.toHaveBeenCalled();
  });

  it('ignores an editor state response after the document context changes', async () => {
    const { coordinator, document, applyEditorSessionForContext } = setup();
    document.matchesCommandContext = () => false;

    const outcome = await coordinator.refreshEditorState();

    expect(outcome).toEqual({ status: 'stale' });
    expect(applyEditorSessionForContext).not.toHaveBeenCalled();
  });
});
