import { describe, expect, it, vi } from 'vitest';

import { createPendingCellSaveCoordinator } from '@/application/pendingCellSaveCoordinator';
import { usePendingCellSavesStore } from '@/stores/pendingCellSaves';
import type { CellSaveRequest, PendingCellSaveCallbacks } from '@/types';
import { createPinia, setActivePinia } from 'pinia';

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

function request(value: string): CellSaveRequest {
  return {
    sheetIndex: 0,
    row: 0,
    col: 0,
    value,
    oldValue: { type: 'cell', kind: 'text', raw: '', display: '' },
  };
}

describe('pendingCellSaveCoordinator', () => {
  it('keeps an in-flight save tracked across reset and serializes the next generation', async () => {
    setActivePinia(createPinia());
    const store = usePendingCellSavesStore();
    const coordinator = createPendingCellSaveCoordinator(store);
    const releaseOldSave = deferred<void>();
    const releaseNewSave = deferred<void>();
    let active = 0;
    let peak = 0;
    const oldCallbacks: PendingCellSaveCallbacks = {
      commitBatch: vi.fn(async () => {
        active += 1;
        peak = Math.max(peak, active);
        await releaseOldSave.promise;
        active -= 1;
      }),
      clearPendingContentChange: vi.fn(),
    };
    const newCallbacks: PendingCellSaveCallbacks = {
      commitBatch: vi.fn(async () => {
        active += 1;
        peak = Math.max(peak, active);
        await releaseNewSave.promise;
        active -= 1;
      }),
      clearPendingContentChange: vi.fn(),
    };

    store.queueSave('0:0:0', request('old'));
    coordinator.startPendingSave(oldCallbacks);
    await flushPromises();
    coordinator.reset();
    const oldDrain = coordinator.waitForInFlightSave();
    store.queueSave('0:0:0', request('new'));
    coordinator.startPendingSave(newCallbacks);
    await flushPromises();

    expect(newCallbacks.commitBatch).not.toHaveBeenCalled();

    releaseOldSave.resolve();
    await expect(oldDrain).resolves.toBe(false);
    await flushPromises();
    expect(newCallbacks.commitBatch).toHaveBeenCalledOnce();

    releaseNewSave.resolve();
    await expect(coordinator.waitForInFlightSave()).resolves.toBe(true);
    expect(peak).toBe(1);
  });

  it('continues a flush after the save it awaited is retired by reset', async () => {
    setActivePinia(createPinia());
    const store = usePendingCellSavesStore();
    const coordinator = createPendingCellSaveCoordinator(store);
    const releaseOldSave = deferred<void>();
    const oldCallbacks: PendingCellSaveCallbacks = {
      commitBatch: vi.fn(() => releaseOldSave.promise),
      clearPendingContentChange: vi.fn(),
    };
    const newCallbacks: PendingCellSaveCallbacks = {
      commitBatch: vi.fn().mockResolvedValue(undefined),
      clearPendingContentChange: vi.fn(),
    };

    store.queueSave('0:0:0', request('old'));
    coordinator.startPendingSave(oldCallbacks);
    await flushPromises();
    coordinator.reset();
    store.queueSave('0:0:0', request('new'));
    const flush = coordinator.flushPendingCellChanges(newCallbacks);
    releaseOldSave.resolve();

    await expect(flush).resolves.toBe(true);
    expect(newCallbacks.commitBatch).toHaveBeenCalledOnce();
  });
});
