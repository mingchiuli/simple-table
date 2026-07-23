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
});
