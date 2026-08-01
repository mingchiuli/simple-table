import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { createDocumentWorkspaceRuntime } from '@/composables/documentWorkspaceRuntime';
import { useDocumentCommandBus } from '@/composables/useDocumentCommandBus';
import { useDocumentSessionCoordinator } from '@/composables/useDocumentSessionCoordinator';
import { usePendingCellSaveCoordinator } from '@/composables/usePendingCellSaveCoordinator';
import { useSearchSessionCoordinator } from '@/composables/useSearchSessionCoordinator';
import {
  createDocumentWorkspaceTestContext,
  type DocumentWorkspaceTestContext,
} from '@/test/documentWorkspaceTestContext';
import { WorkspaceDisposedError } from '@/application/workspaceOperationTracker';
import { createDocumentFileOperationProtocol } from '@/application/documentFileOperationProtocol';
import {
  createOperationCancellationSource,
  OperationCancelledError,
} from '@/application/operationCancellation';

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((promiseResolve) => {
    resolve = promiseResolve;
  });
  return { promise, resolve };
}

describe('documentWorkspaceRuntime', () => {
  let workspace: DocumentWorkspaceTestContext;

  beforeEach(() => {
    setActivePinia(createPinia());
    workspace = createDocumentWorkspaceTestContext();
  });

  it('owns every document-scoped coordinator as one runtime', () => {
    const { runtime } = workspace;

    expect(createDocumentWorkspaceRuntime()).not.toBe(runtime);
    workspace.run(() => {
      expect(useDocumentSessionCoordinator()).toBe(runtime.session);
      expect(useDocumentCommandBus()).toBe(runtime.commandBus);
      expect(usePendingCellSaveCoordinator()).toBe(runtime.pendingCellSaves);
      expect(useSearchSessionCoordinator()).toBe(runtime.search);
    });
  });

  it('creates isolated runtimes even for the same Pinia document store', () => {
    const first = createDocumentWorkspaceRuntime();
    const second = createDocumentWorkspaceRuntime();

    expect(second).not.toBe(first);
    expect(second.document).toBe(first.document);
    expect(second.session).not.toBe(first.session);
    expect(second.preparations).not.toBe(first.preparations);
  });

  it('rejects coordinator access outside the application injection context', () => {
    expect(() => useDocumentSessionCoordinator()).toThrow(
      'Document workspace runtime must be provided by the application root',
    );
  });

  it('drains document preparation before releasing the runtime', async () => {
    const runtime = createDocumentWorkspaceRuntime();
    let releasePreparation!: () => void;
    const preparation = runtime.preparations.run(() => new Promise<void>((resolve) => {
      releasePreparation = resolve;
    }));
    let disposed = false;
    const disposal = runtime.dispose().then(() => {
      disposed = true;
    });

    await Promise.resolve();
    expect(disposed).toBe(false);
    releasePreparation();
    await Promise.all([preparation, disposal]);

    await expect(runtime.dispose()).resolves.toBeUndefined();
  });

  it('retries retained preparation ID cleanup before releasing the runtime', async () => {
    const runtime = createDocumentWorkspaceRuntime();
    const finalCleanup = deferred<void>();
    const cleanupFailure = new Error('cleanup unavailable');
    const discard = vi
      .fn()
      .mockRejectedValueOnce(cleanupFailure)
      .mockRejectedValueOnce(cleanupFailure)
      .mockRejectedValueOnce(cleanupFailure)
      .mockImplementationOnce(() => finalCleanup.promise);

    await expect(runtime.preparations.cleanupPreparationId('preparation-1', discard))
      .resolves.toBe(false);
    let disposed = false;
    const disposal = runtime.dispose().then(() => { disposed = true; });
    for (let index = 0; index < 8 && discard.mock.calls.length < 4; index += 1) {
      await Promise.resolve();
    }

    expect(discard).toHaveBeenCalledTimes(4);
    expect(disposed).toBe(false);

    finalCleanup.resolve();
    await disposal;
    expect(disposed).toBe(true);
  });

  it('waits for admitted workspace operations and rejects work after disposal starts', async () => {
    const runtime = createDocumentWorkspaceRuntime();
    const active = deferred<string>();
    const operation = runtime.runRequiredTask(() => active.promise);
    let disposed = false;
    const disposal = runtime.dispose().then(() => { disposed = true; });
    const lateTask = vi.fn().mockResolvedValue('late');

    await expect(runtime.runTask(lateTask, 'disposed')).resolves.toBe('disposed');
    await expect(runtime.runRequiredTask(lateTask)).rejects.toBeInstanceOf(WorkspaceDisposedError);
    await expect(runtime.preparations.run(lateTask)).rejects.toBeInstanceOf(
      WorkspaceDisposedError,
    );
    expect(lateTask).not.toHaveBeenCalled();
    expect(disposed).toBe(false);

    active.resolve('completed');
    await expect(operation).resolves.toBe('completed');
    await disposal;
    expect(disposed).toBe(true);
    await expect(runtime.commandBus.ensureSheetRegionLoaded({
      sheetIndex: 0,
      rowStart: 0,
      rowEnd: 1,
      colStart: 0,
      colEnd: 1,
    })).resolves.toBe(false);
  });

  it('lets admitted operations finish internal work after disposal starts', async () => {
    const runtime = createDocumentWorkspaceRuntime();
    const active = deferred<void>();
    const prepare = vi.fn().mockResolvedValue('prepared');
    const operation = runtime.runTask(async ({ preparations }) => {
      await active.promise;
      return preparations.run(prepare);
    }, 'disposed');

    const disposal = runtime.dispose();
    active.resolve();

    await expect(operation).resolves.toBe('prepared');
    await disposal;
    expect(prepare).toHaveBeenCalledOnce();
  });

  it('cancels non-terminal operation recovery so disposal can finish', async () => {
    const runtime = createDocumentWorkspaceRuntime();
    const lookupStarted = deferred<void>();
    const operation = runtime.runTask(({ cancellation }) => {
      const protocol = createDocumentFileOperationProtocol({
        getFileOperationResult: () => {
          lookupStarted.resolve();
          return new Promise(() => undefined);
        },
        cancellation,
      });
      return protocol.execute({
        kind: 'export',
        invoke: vi.fn().mockRejectedValue(new Error('response lost')),
        receiptForResponse: () => null,
        validateReceipt: () => true,
        recoverResponse: async () => null,
        recoverCancelled: () => null,
      });
    }, null);

    await lookupStarted.promise;
    const disposal = runtime.dispose();

    await expect(operation).rejects.toBeInstanceOf(OperationCancelledError);
    await expect(disposal).resolves.toBeUndefined();
  });

  it('drains all admitted work and reaches disposal after preparation cleanup fails', async () => {
    const runtime = createDocumentWorkspaceRuntime();
    const preparation = deferred<string>();
    const preparationStarted = deferred<void>();
    const active = deferred<void>();
    const cancellation = createOperationCancellationSource();
    const discard = vi.fn().mockRejectedValue(new Error('cleanup failed'));
    const cancelledPreparation = runtime.preparations.runCancellable(
      () => {
        preparationStarted.resolve();
        return preparation.promise;
      },
      cancellation.signal,
      discard,
    );
    const activeOperation = runtime.runRequiredTask(() => active.promise);

    await preparationStarted.promise;
    cancellation.cancel();
    await expect(cancelledPreparation).resolves.toBeUndefined();

    let settled = false;
    const disposal = runtime.dispose();
    void disposal.then(
      () => { settled = true; },
      () => { settled = true; },
    );
    preparation.resolve('prepared');
    for (let index = 0; index < 12 && discard.mock.calls.length < 3; index += 1) {
      await Promise.resolve();
    }

    expect(discard).toHaveBeenCalledTimes(3);
    expect(settled).toBe(false);

    active.resolve();
    await expect(activeOperation).resolves.toBeUndefined();
    await expect(disposal).rejects.toMatchObject({
      name: 'AggregateError',
      message: 'Failed to completely drain the document workspace',
    });
    await expect(runtime.dispose()).rejects.toMatchObject({ name: 'AggregateError' });
    await expect(runtime.runRequiredTask(() => Promise.resolve())).rejects.toBeInstanceOf(
      WorkspaceDisposedError,
    );
  });

  it('notifies every cancellation observer before reporting disposal failure', async () => {
    const runtime = createDocumentWorkspaceRuntime();
    const cancellationFailure = new Error('cancellation observer failed');
    const finalObserver = vi.fn();
    await runtime.runTask(async ({ cancellation }) => {
      cancellation.onCancel(() => {
        throw cancellationFailure;
      });
      cancellation.onCancel(finalObserver);
    }, undefined);

    await expect(runtime.dispose()).rejects.toMatchObject({
      name: 'AggregateError',
      message: 'Failed to completely drain the document workspace',
    });
    expect(finalObserver).toHaveBeenCalledOnce();
  });
});
