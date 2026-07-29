import { describe, expect, it, vi } from 'vitest';

import { createDocumentFileOperationProtocol } from '@/application/documentFileOperationProtocol';
import type { FileOperationReceipt } from '@/types/fileRuntime';

const receipt: FileOperationReceipt = {
  kind: 'save',
  documentId: '7',
  revision: '4',
  path: '/tmp/book.xlsx',
  fileName: 'book.xlsx',
};

describe('documentFileOperationProtocol', () => {
  it('does not retry a definitive backend rejection', async () => {
    const rejection = { code: 'document_state_invalid', message: 'revision changed' };
    const invoke = vi.fn().mockRejectedValue(rejection);
    const getFileOperationResult = vi.fn();
    const protocol = createDocumentFileOperationProtocol({
      getFileOperationResult,
      createOperationId: () => 'operation-definitive',
    });

    await expect(protocol.execute<{ receipt: FileOperationReceipt }>({
      kind: 'save',
      invoke,
      receiptForResponse: (response) => response.receipt,
      validateReceipt: () => true,
      recoverResponse: vi.fn(),
    })).rejects.toBe(rejection);

    expect(invoke).toHaveBeenCalledOnce();
    expect(getFileOperationResult).not.toHaveBeenCalled();
  });

  it('does not retry a response that fails protocol admission', async () => {
    const invoke = vi.fn().mockResolvedValue({ receipt });
    const protocol = createDocumentFileOperationProtocol({
      getFileOperationResult: vi.fn(),
      createOperationId: () => 'operation-mismatch',
    });

    await expect(protocol.execute<{ receipt: FileOperationReceipt }>({
      kind: 'save',
      invoke,
      receiptForResponse: (response) => response.receipt,
      validateReceipt: () => false,
      recoverResponse: vi.fn(),
    })).rejects.toThrow('mismatched save operation receipt');

    expect(invoke).toHaveBeenCalledOnce();
  });

  it('retries an ambiguous request with the same operation id', async () => {
    const invoke = vi.fn()
      .mockRejectedValueOnce(new Error('response lost'))
      .mockResolvedValueOnce({ receipt });
    const protocol = createDocumentFileOperationProtocol({
      getFileOperationResult: vi.fn(),
      createOperationId: () => 'operation-1',
    });

    await expect(protocol.execute<{ receipt: FileOperationReceipt }>({
      kind: 'save',
      invoke,
      receiptForResponse: (response) => response.receipt,
      validateReceipt: (value) => value.documentId === '7',
      recoverResponse: vi.fn(),
    })).resolves.toEqual({ receipt });

    expect(invoke).toHaveBeenNthCalledWith(1, 'operation-1');
    expect(invoke).toHaveBeenNthCalledWith(2, 'operation-1');
  });

  it('recovers a completed response after both invokes lose their response', async () => {
    const recoverResponse = vi.fn().mockResolvedValue({ receipt, recovered: true });
    const getFileOperationResult = vi.fn()
      .mockResolvedValueOnce({ status: 'pending' })
      .mockResolvedValueOnce({ status: 'completed', receipt });
    let now = 0;
    const protocol = createDocumentFileOperationProtocol({
      getFileOperationResult,
      createOperationId: () => 'operation-2',
      clock: {
        now: () => now,
        sleep: async (milliseconds) => { now += milliseconds; },
      },
    });

    await expect(protocol.execute<{ receipt: FileOperationReceipt }>({
      kind: 'save',
      invoke: vi.fn().mockRejectedValue(new Error('response lost')),
      receiptForResponse: (response) => response.receipt,
      validateReceipt: (value) => value.documentId === '7',
      recoverResponse,
    })).resolves.toEqual({ receipt, recovered: true });

    expect(getFileOperationResult).toHaveBeenCalledTimes(2);
    expect(recoverResponse).toHaveBeenCalledWith(receipt);
  });

  it('continues polling an admitted operation until it reaches a terminal result', async () => {
    const recoverResponse = vi.fn().mockResolvedValue({ receipt, recovered: true });
    let now = 0;
    const getFileOperationResult = vi.fn(async () => now < 5_000
      ? { status: 'pending' as const }
      : { status: 'completed' as const, receipt });
    const protocol = createDocumentFileOperationProtocol({
      getFileOperationResult,
      createOperationId: () => 'operation-long-running',
      clock: {
        now: () => now,
        sleep: async (milliseconds) => { now += milliseconds; },
      },
    });

    await expect(protocol.execute<{ receipt: FileOperationReceipt }>({
      kind: 'save',
      invoke: vi.fn().mockRejectedValue(new Error('response lost')),
      receiptForResponse: (response) => response.receipt,
      validateReceipt: () => true,
      recoverResponse,
    })).resolves.toEqual({ receipt, recovered: true });

    expect(now).toBeGreaterThanOrEqual(5_000);
    expect(recoverResponse).toHaveBeenCalledWith(receipt);
  });

  it('surfaces a terminal backend failure after an ambiguous response', async () => {
    const failure = { code: 'write_error', message: 'disk full' };
    const recoverAmbiguous = vi.fn();
    const protocol = createDocumentFileOperationProtocol({
      getFileOperationResult: vi.fn().mockResolvedValue({
        status: 'failed',
        error: failure,
      }),
      createOperationId: () => 'operation-failed',
    });

    await expect(protocol.execute<{ receipt: FileOperationReceipt }>({
      kind: 'save',
      invoke: vi.fn().mockRejectedValue(new Error('response lost')),
      receiptForResponse: (response) => response.receipt,
      validateReceipt: () => true,
      recoverResponse: vi.fn(),
      recoverAmbiguous,
    })).rejects.toBe(failure);

    expect(recoverAmbiguous).not.toHaveBeenCalled();
  });

  it('recovers a terminal cancellation without repeating the side effect', async () => {
    const protocol = createDocumentFileOperationProtocol({
      getFileOperationResult: vi.fn().mockResolvedValue({ status: 'cancelled' }),
      createOperationId: () => 'operation-cancelled',
    });

    await expect(protocol.execute<FileOperationReceipt | null>({
      kind: 'export',
      invoke: vi.fn().mockRejectedValue(new Error('response lost')),
      receiptForResponse: (response) => response,
      validateReceipt: () => true,
      recoverResponse: async (value) => value,
      recoverCancelled: () => null,
    })).resolves.toBeNull();
  });
});
