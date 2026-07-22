import { beforeEach, describe, expect, it, vi } from 'vitest';
import { createPinia, setActivePinia } from 'pinia';

import {
  createDocumentWorkspaceTestContext,
  type DocumentWorkspaceTestContext,
} from '@/test/documentWorkspaceTestContext';

const apiMocks = vi.hoisted(() => ({
  getEditorState: vi.fn(),
}));

vi.mock('element-plus', () => ({
  ElMessage: { error: vi.fn() },
}));

vi.mock('@/api', () => apiMocks);

describe('useDocumentCommandBus', () => {
  let workspace: DocumentWorkspaceTestContext;

  beforeEach(() => {
    setActivePinia(createPinia());
    workspace = createDocumentWorkspaceTestContext();
    apiMocks.getEditorState.mockReset().mockResolvedValue(null);
  });

  it('keeps command facades scoped to their explicit document runtime', () => {
    const first = workspace.runtime.commandBus;

    setActivePinia(createPinia());
    const secondWorkspace = createDocumentWorkspaceTestContext();

    const second = secondWorkspace.runtime.commandBus;
    expect(second).not.toBe(first);
  });

  it('propagates current-context editor state refresh failures', async () => {
    const error = new Error('status unavailable');
    apiMocks.getEditorState.mockRejectedValue(error);

    await expect(workspace.runtime.commandBus.refreshEditorState()).rejects.toBe(error);
  });
});
