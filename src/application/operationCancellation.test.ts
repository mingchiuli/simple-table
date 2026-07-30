import { describe, expect, it, vi } from 'vitest';

import {
  createOperationCancellationSource,
  throwIfOperationCancellationFailed,
} from '@/application/operationCancellation';

describe('operation cancellation', () => {
  it('notifies every observer and reports failures after the broadcast', () => {
    const source = createOperationCancellationSource();
    const firstFailure = new Error('first observer failed');
    const finalObserver = vi.fn();
    source.signal.onCancel(() => {
      throw firstFailure;
    });
    source.signal.onCancel(finalObserver);

    const failures = source.cancel();

    expect(finalObserver).toHaveBeenCalledOnce();
    expect(failures).toEqual([firstFailure]);
    expect(source.cancel()).toBe(failures);
    expect(() => throwIfOperationCancellationFailed(
      failures,
      'Cancellation notification failed',
    )).toThrow(expect.objectContaining({
      name: 'AggregateError',
      message: 'Cancellation notification failed',
      errors: [firstFailure],
    }));
  });
});
