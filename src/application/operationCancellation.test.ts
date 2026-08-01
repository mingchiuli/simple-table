import { describe, expect, it, vi } from 'vitest';

import {
  createOperationCancellationSource,
  OperationCancelledError,
  raceWithOperationCancellation,
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

  it('does not start an operation after cancellation', async () => {
    const source = createOperationCancellationSource();
    const start = vi.fn(async () => 'result');
    source.cancel();

    await expect(raceWithOperationCancellation(start, source.signal))
      .rejects.toBeInstanceOf(OperationCancelledError);

    expect(start).not.toHaveBeenCalled();
  });

  it('does not start when cancellation happens while registering the observer', async () => {
    const start = vi.fn(async () => 'result');
    const cancellation = {
      isCancelled: () => false,
      onCancel(handler: () => void) {
        handler();
        return () => undefined;
      },
    };

    await expect(raceWithOperationCancellation(start, cancellation))
      .rejects.toBeInstanceOf(OperationCancelledError);

    expect(start).not.toHaveBeenCalled();
  });

  it('contains failures from observers registered after cancellation', () => {
    const source = createOperationCancellationSource();
    const failures = source.cancel();
    const lateFailure = new Error('late observer failed');

    expect(() => source.signal.onCancel(() => {
      throw lateFailure;
    })).not.toThrow();
    expect(failures).toEqual([lateFailure]);
  });
});
