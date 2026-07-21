import { beforeEach, describe, expect, it, vi } from 'vitest';
import { createPinia, setActivePinia } from 'pinia';

import { useDocumentCommandBus } from '@/composables/useDocumentCommandBus';

const apiMocks = vi.hoisted(() => ({
  getEditorState: vi.fn(),
}));

vi.mock('element-plus', () => ({
  ElMessage: { error: vi.fn() },
}));

vi.mock('@/api', () => apiMocks);

describe('useDocumentCommandBus', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    apiMocks.getEditorState.mockReset().mockResolvedValue(null);
  });

  it('returns one command facade per Pinia document session', () => {
    const first = useDocumentCommandBus();
    expect(useDocumentCommandBus()).toBe(first);

    setActivePinia(createPinia());

    const second = useDocumentCommandBus();
    expect(useDocumentCommandBus()).toBe(second);
    expect(second).not.toBe(first);
  });

  it('propagates current-context editor state refresh failures', async () => {
    const error = new Error('status unavailable');
    apiMocks.getEditorState.mockRejectedValue(error);

    await expect(useDocumentCommandBus().refreshEditorState()).rejects.toBe(error);
  });
});
