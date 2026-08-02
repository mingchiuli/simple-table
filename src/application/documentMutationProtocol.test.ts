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
import { OperationOutcomeUnknownError } from '@/application/operationOutcome';
import type { OperationCancellationSignal } from '@/application/operationCancellation';
import { defaultWorkbookCapabilities, readyFormulaStatus } from '@/types';
import {
  createOperationCancellationSource,
  OperationCancelledError,
} from '@/application/operationCancellation';

describe('document mutation protocol', () => {
  it('does not invoke a mutation after its workspace is already disposed', async () => {
    const cancellation = createOperationCancellationSource();
    const action = vi.fn().mockResolvedValue(mutationResponse('2'));
    const protocol = createProtocol({ cancellation: cancellation.signal });
    cancellation.cancel();

    await expect(protocol.execute(action, context()))
      .rejects.toBeInstanceOf(OperationCancelledError);

    expect(action).not.toHaveBeenCalled();
  });

  it('does not retry a definitive backend rejection', async () => {
    const rejection = { code: 'document_state_invalid', message: 'revision changed' };
    const action = vi.fn().mockRejectedValue(rejection);
    const getMutationResult = vi.fn();
    const protocol = createProtocol({ getMutationResult });

    await expect(protocol.execute(action, context())).rejects.toBe(rejection);

    expect(action).toHaveBeenCalledOnce();
    expect(getMutationResult).not.toHaveBeenCalled();
  });

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

  it('refreshes an advanced projection without claiming the ambiguous command succeeded', async () => {
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

    const transportError = new Error('ipc closed');
    await expect(protocol.execute(
      vi.fn().mockRejectedValue(transportError),
      context()
    )).rejects.toBe(transportError);

    expect(recoverProjection).toHaveBeenCalledWith(projection, 3);
  });

  it('continues polling an admitted mutation beyond the discovery deadline', async () => {
    const replay = mutationResponse('2');
    let now = 0;
    const protocol = createProtocol({
      getMutationResult: vi.fn(async () => now < 5_000
        ? { status: 'pending' } as MutationResultLookup
        : { status: 'completed', response: replay } as MutationResultLookup),
      clock: {
        now: () => now,
        sleep: async (milliseconds) => { now += milliseconds; },
      },
    });

    await expect(protocol.execute(
      vi.fn().mockRejectedValue(new Error('ipc closed')),
      context(),
    )).resolves.toEqual({ status: 'response', response: replay });
    expect(now).toBeGreaterThanOrEqual(5_000);
  });

  it('stops polling a permanently pending mutation and locks its projection', async () => {
    let now = 0;
    const markOutcomeUnknown = vi.fn();
    const protocol = createProtocol({
      getMutationResult: vi.fn(async () => ({ status: 'pending' } as MutationResultLookup)),
      markOutcomeUnknown,
      terminalResultTimeoutMs: 100,
      clock: {
        now: () => now,
        sleep: async (milliseconds) => { now += milliseconds; },
      },
    });

    await expect(protocol.execute(
      vi.fn().mockRejectedValue(new Error('ipc closed')),
      context(),
    )).rejects.toBeInstanceOf(OperationOutcomeUnknownError);

    expect(markOutcomeUnknown).toHaveBeenCalledWith(context());
    expect(now).toBeGreaterThanOrEqual(100);
  });

  it('surfaces a terminal mutation failure after an ambiguous response', async () => {
    const failure = { code: 'document_state_invalid', message: 'revision changed' };
    const protocol = createProtocol({
      getMutationResult: vi.fn(async () => ({
        status: 'failed',
        error: failure,
      } as MutationResultLookup)),
    });

    await expect(protocol.execute(
      vi.fn().mockRejectedValue(new Error('ipc closed')),
      context(),
    )).rejects.toBe(failure);
  });

  it('stops waiting for a non-terminal mutation when its workspace is disposed', async () => {
    const cancellation = createOperationCancellationSource();
    const lookupStarted = deferred<void>();
    const protocol = createProtocol({
      cancellation: cancellation.signal,
      getMutationResult: vi.fn(() => {
        lookupStarted.resolve();
        return new Promise<MutationResultLookup>(() => undefined);
      }),
    });
    const execution = protocol.execute(
      vi.fn().mockRejectedValue(new Error('ipc closed')),
      context(),
    );

    await lookupStarted.promise;
    cancellation.cancel();

    await expect(execution).rejects.toBeInstanceOf(OperationCancelledError);
  });

  it('stops active-document recovery when its workspace is disposed', async () => {
    const cancellation = createOperationCancellationSource();
    const recoveryStarted = deferred<void>();
    const protocol = createProtocol({
      cancellation: cancellation.signal,
      getActiveDocument: vi.fn(() => {
        recoveryStarted.resolve();
        return new Promise<OpenDocumentResponse | null>(() => undefined);
      }),
    });
    const execution = protocol.execute(
      vi.fn().mockRejectedValue(new Error('ipc closed')),
      context(),
    );

    await recoveryStarted.promise;
    cancellation.cancel();

    await expect(execution).rejects.toBeInstanceOf(OperationCancelledError);
  });

  it('stops projection recovery when its workspace is disposed', async () => {
    const cancellation = createOperationCancellationSource();
    const recoveryStarted = deferred<void>();
    const protocol = createProtocol({
      cancellation: cancellation.signal,
      getActiveDocument: vi.fn(async () => openResponse('2')),
      getCurrentDocumentProjection: vi.fn(() => {
        recoveryStarted.resolve();
        return new Promise<OpenDocumentResponse>(() => undefined);
      }),
    });
    const execution = protocol.execute(
      vi.fn().mockRejectedValue(new Error('ipc closed')),
      context(),
    );

    await recoveryStarted.promise;
    cancellation.cancel();

    await expect(execution).rejects.toBeInstanceOf(OperationCancelledError);
  });
});

type ProtocolOverrides = Partial<DocumentMutationTransport> & {
  recoverProjection?: DocumentMutationRecovery['recoverProjection'];
  markOutcomeUnknown?: DocumentMutationRecovery['markOutcomeUnknown'];
  clock?: {
    now: () => number;
    sleep: (milliseconds: number) => Promise<void>;
  };
  cancellation?: OperationCancellationSignal;
  terminalResultTimeoutMs?: number;
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
      markOutcomeUnknown: overrides.markOutcomeUnknown,
    },
    createCommandId: () => 'command-1',
    clock: overrides.clock ?? advancingClock(),
    cancellation: overrides.cancellation,
    terminalResultTimeoutMs: overrides.terminalResultTimeoutMs,
  });
}

function deferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void;
  const promise = new Promise<T>((promiseResolve) => {
    resolve = promiseResolve;
  });
  return { promise, resolve };
}

function advancingClock() {
  let now = 0;
  return {
    now: () => now,
    sleep: async (milliseconds: number) => { now += milliseconds; },
  };
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
