import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it } from 'vitest';

import { createDocumentWorkspaceRuntime } from '@/composables/documentWorkspaceRuntime';
import { useDocumentCommandBus } from '@/composables/useDocumentCommandBus';
import { useDocumentSessionCoordinator } from '@/composables/useDocumentSessionCoordinator';
import { usePendingCellSaveCoordinator } from '@/composables/usePendingCellSaveCoordinator';
import { useSearchSessionCoordinator } from '@/composables/useSearchSessionCoordinator';
import {
  createDocumentWorkspaceTestContext,
  type DocumentWorkspaceTestContext,
} from '@/test/documentWorkspaceTestContext';

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
});
