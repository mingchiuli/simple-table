import { describe, expect, it, vi } from 'vitest';
import {
  createDocumentMutationProtocol,
  type DocumentMutationRecovery,
  type DocumentMutationTransport,
} from '@/application/documentMutationProtocol';
import type {
  EditorMutationResponse,
  MutationResultLookup,
  OpenDocumentResponse,
} from '@/types/protocol';
import { defaultWorkbookCapabilities, readyFormulaStatus } from '@/types';

describe('document mutation protocol', () => {
  it('retries an ambiguous mutation with the same command id', async () => {
    const action = vi.fn()
      .mockRejectedValueOnce(new Error('ipc closed'))
      .mockResolvedValueOnce(mutationResponse('2'));
    const protocol = createProtocol();

    const result = await protocol.execute(action, context());

    expect(result.status).toBe('response');
    expect(action).toHaveBeenCalledTimes(2);
    expect(action.mock.calls[0][0].commandId).toBe('command-1');
    expect(action.mock.calls[1][0].commandId).toBe('command-1');
  });

  it('returns a completed replay after both IPC attempts fail', async () => {
    const replay = mutationResponse('2');
    const getMutationResult = vi.fn()
      .mockResolvedValueOnce({ status: 'pending' } satisfies MutationResultLookup)
      .mockResolvedValueOnce({
        status: 'completed',
        response: replay,
      } satisfies MutationResultLookup);
    let now = 0;
    const protocol = createProtocol({
      getMutationResult,
      clock: {
        now: () => now,
        sleep: async (milliseconds: number) => {
          now += milliseconds;
        },
      },
    });

    const result = await protocol.execute(
      vi.fn().mockRejectedValue(new Error('ipc closed')),
      context()
    );

    expect(result).toEqual({ status: 'response', response: replay });
    expect(getMutationResult).toHaveBeenCalledTimes(2);
  });

  it('reports recovery when the active revision advanced', async () => {
    const projection = openResponse('2');
    const recoverProjection = vi.fn(() => true);
    const protocol = createProtocol({
      getMutationResult: vi.fn(async () => (
        { status: 'missing' } satisfies MutationResultLookup
      )),
      getActiveDocument: vi.fn(async () => projection),
      getCurrentDocumentProjection: vi.fn(async () => projection),
      recoverProjection,
    });

    const result = await protocol.execute(
      vi.fn().mockRejectedValue(new Error('ipc closed')),
      context()
    );

    expect(result).toEqual({ status: 'recovered' });
    expect(recoverProjection).toHaveBeenCalledWith(projection, 3);
  });
});

type ProtocolOverrides = Partial<DocumentMutationTransport> & {
  recoverProjection?: DocumentMutationRecovery['recoverProjection'];
  clock?: {
    now: () => number;
    sleep: (milliseconds: number) => Promise<void>;
  };
};

function createProtocol(overrides: ProtocolOverrides = {}) {
  const transport = {
    getMutationResult: overrides.getMutationResult
      ?? vi.fn(async () => ({ status: 'missing' } as MutationResultLookup)),
    getActiveDocument: overrides.getActiveDocument ?? vi.fn(async () => null),
    getCurrentDocumentProjection: overrides.getCurrentDocumentProjection
      ?? vi.fn(async () => openResponse('1')),
  } satisfies DocumentMutationTransport;
  return createDocumentMutationProtocol({
    transport,
    recovery: {
      preferredSheetIndex: () => 3,
      recoverProjection: overrides.recoverProjection ?? (() => false),
    },
    createCommandId: () => 'command-1',
    clock: overrides.clock ?? { now: () => 0, sleep: async () => undefined },
  });
}

function context() {
  return { documentId: '7' as const, baseRevision: '1' as const };
}

function mutationResponse(revision: `${bigint}`): EditorMutationResponse {
  return {
    protocolVersion: 4,
    documentId: '7',
    revision,
    formulaStatus: readyFormulaStatus(),
    capabilities: defaultWorkbookCapabilities(),
    editorState: {
      canUndo: true,
      canRedo: false,
      isDirty: true,
      history: {
        isTruncated: false,
        undoEntries: 1,
        redoEntries: 0,
        undoEstimatedBytes: 0,
        redoEstimatedBytes: 0,
        maxHistoryBytes: 0,
        maxSingleEntryBytes: 0,
      },
    },
  };
}

function openResponse(revision: `${bigint}`): OpenDocumentResponse {
  return {
    document: { path: '', fileName: 'book.xlsx', sheets: [] },
    editorSession: {
      documentId: '7',
      revision,
      formulaStatus: readyFormulaStatus(),
      capabilities: defaultWorkbookCapabilities(),
      editorState: mutationResponse(revision).editorState,
    },
  };
}
