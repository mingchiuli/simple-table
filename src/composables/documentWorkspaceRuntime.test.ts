import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it } from 'vitest';

import { createDocumentWorkspaceRuntime } from '@/composables/documentWorkspaceRuntime';
import { useDocumentCommandBus } from '@/composables/useDocumentCommandBus';
import { useDocumentSessionCoordinator } from '@/composables/useDocumentSessionCoordinator';
import { usePendingCellSaveCoordinator } from '@/composables/usePendingCellSaveCoordinator';
import { useSearchSessionCoordinator } from '@/composables/useSearchSessionCoordinator';

describe('documentWorkspaceRuntime', () => {
  beforeEach(() => setActivePinia(createPinia()));

  it('owns every document-scoped coordinator as one runtime', () => {
    const runtime = createDocumentWorkspaceRuntime();

    expect(createDocumentWorkspaceRuntime()).toBe(runtime);
    expect(useDocumentSessionCoordinator()).toBe(runtime.session);
    expect(useDocumentCommandBus()).toBe(runtime.commandBus);
    expect(usePendingCellSaveCoordinator()).toBe(runtime.pendingCellSaves);
    expect(useSearchSessionCoordinator()).toBe(runtime.search);
  });

  it('creates an isolated runtime for a different Pinia document store', () => {
    const first = createDocumentWorkspaceRuntime();
    setActivePinia(createPinia());
    const second = createDocumentWorkspaceRuntime();

    expect(second).not.toBe(first);
    expect(second.session).not.toBe(first.session);
    expect(second.preparations).not.toBe(first.preparations);
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

    expect(createDocumentWorkspaceRuntime()).not.toBe(runtime);
  });
});
