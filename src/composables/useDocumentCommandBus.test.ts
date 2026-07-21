import { beforeEach, describe, expect, it, vi } from 'vitest';
import { createPinia, setActivePinia } from 'pinia';

import { useDocumentCommandBus } from '@/composables/useDocumentCommandBus';

vi.mock('element-plus', () => ({
  ElMessage: { error: vi.fn() },
}));

vi.mock('@/api', () => ({}));

describe('useDocumentCommandBus', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
  });

  it('returns one command facade per Pinia document session', () => {
    const first = useDocumentCommandBus();
    expect(useDocumentCommandBus()).toBe(first);

    setActivePinia(createPinia());

    const second = useDocumentCommandBus();
    expect(useDocumentCommandBus()).toBe(second);
    expect(second).not.toBe(first);
  });
});
